//! Dry run of a core in a child process (design 8.0 §2.5, ADR 0025 §5).
//!
//! The parent ([`run_dry_run`]) writes a request file (toml) in the dry-run folder, starts the executable itself
//! with `--dry-run-child <request>` (standard streams to null), waits at most a timeout and reads a bounded
//! result file. The child ([`dry_run_child_main`]) runs `emu_runtime::dry_run` and writes the result; it ends by
//! itself after a watchdog delay, even when the parent is dead. A child that crashes, hangs, or ends with code 0
//! without a readable result is a failed dry run, never a failure of the parent.
//!
//! The serialisable (toml) mirror structures of `DryRunRequest` / `DryRunReport` are private to this module:
//! `emu-runtime` and `emu-libretro` stay free of `serde`.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use emu_runtime::{
    DryRunOutcome, DryRunReport, DryRunRequest, HwContextKind, HwRenderRequest, InputDescriptor,
    MAX_INPUT_DESCRIPTORS,
};
use serde::{Deserialize, Serialize};

/// Delay after which the parent kills the child (ADR 0025 §5).
pub const DRY_RUN_TIMEOUT: Duration = Duration::from_secs(30);
/// Frames run by a dry run (ADR 0025 §5).
pub const DRY_RUN_FRAMES: u32 = 300;
/// Games bigger than this are not tried (the copy would be too long and too big).
pub const MAX_GAME_COPY_BYTES: u64 = 256 * 1024 * 1024;
/// Delay after which the child ends by itself.
const WATCHDOG_SECS: u64 = 35;
/// Largest request or result file read, by the child and by the parent.
const MAX_FILE_BYTES: u64 = 64 * 1024;
/// Largest value of `frames` the child accepts.
const MAX_FRAMES: u32 = 10_000;

const REQUEST_FILE: &str = "request.toml";
const RESULT_FILE: &str = "result.toml";
const LOCK_FILE: &str = ".lock";
const GAME_DIR: &str = "game";
const COPY_DIR: &str = "copies";

/// Child exit codes (informative; the parent only distinguishes 0 with a readable result).
const EXIT_OK: i32 = 0;
const EXIT_BAD_REQUEST: i32 = 2;
const EXIT_WRITE_FAILED: i32 = 3;
/// The watchdog fired.
const EXIT_WATCHDOG: i32 = 4;

/// Bounds applied by the parent to what the child wrote (same limits as the descriptors of the libretro layer).
const MAX_DESCRIPTION_CHARS: usize = 64;
const MAX_NAME_CHARS: usize = 128;
const MAX_MESSAGE_CHARS: usize = 512;
const MAX_EXTENSIONS: usize = 64;
const MAX_EXTENSION_CHARS: usize = 32;

/// Injectable command: tests pass the real binary (`CARGO_BIN_EXE_emu-rust`) and `MOCK_*` variables (no
/// `set_var`). The request file is appended after `args`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl DryRunCommand {
    /// This executable with the hidden `--dry-run-child` argument.
    pub fn current_exe() -> io::Result<Self> {
        Ok(Self {
            program: std::env::current_exe()?,
            args: vec![OsString::from("--dry-run-child")],
            env: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DryRunVerdict {
    Passed(DryRunReport),
    /// The child died (signal, abort: `code` is `None`), failed, or ended with code 0 and no readable result.
    Crashed {
        code: Option<i32>,
    },
    TimedOut,
    HardwareRender(HwRenderRequest),
    LoadFailed(String),
    Skipped(SkipReason),
    /// The child was not started: it could not be spawned, or a path is not valid UTF-8 (no dry run, only the
    /// « non testé » warning).
    SpawnFailed(String),
}

/// Why no dry run was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkipReason {
    /// « Ça marche chez moi » (ADR 0025 §7).
    WorksForMe,
    /// The pair (core, content) already passed.
    AlreadyPassed,
}

// ---------------------------------------------------------------------------------------------------------------
// Files

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionFile {
    key: String,
    value: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestFile {
    core: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<PathBuf>,
    system_dir: PathBuf,
    scratch_dir: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    copy_dir: Option<PathBuf>,
    #[serde(default)]
    options: Vec<OptionFile>,
    frames: u32,
    #[serde(default = "default_watchdog")]
    watchdog_secs: u64,
}

fn default_watchdog() -> u64 {
    WATCHDOG_SECS
}

impl RequestFile {
    fn from_request(request: &DryRunRequest) -> Self {
        Self {
            core: request.core.clone(),
            content: request.content.clone(),
            system_dir: request.system_dir.clone(),
            scratch_dir: request.scratch_dir.clone(),
            copy_dir: request.copy_dir.clone(),
            options: request
                .options
                .iter()
                .map(|(key, value)| OptionFile {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect(),
            frames: request.frames,
            watchdog_secs: WATCHDOG_SECS,
        }
    }

    fn to_request(&self) -> DryRunRequest {
        DryRunRequest {
            core: self.core.clone(),
            content: self.content.clone(),
            system_dir: self.system_dir.clone(),
            scratch_dir: self.scratch_dir.clone(),
            copy_dir: self.copy_dir.clone(),
            options: self
                .options
                .iter()
                .map(|o| (o.key.clone(), o.value.clone()))
                .collect(),
            frames: self.frames.min(MAX_FRAMES),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OutcomeKind {
    Probed,
    Ran,
    LoadFailed,
    HardwareRenderRequired,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HwFile {
    /// `opengl`, `opengles2`, `opengl_core`, `opengles3`, `opengles_version`, `vulkan`, `direct3d11`, `unknown`.
    context: String,
    /// The raw value, for `unknown`.
    raw: u32,
    version_major: u32,
    version_minor: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DescriptorFile {
    port: u32,
    device: u32,
    index: u32,
    id: u32,
    description: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultFile {
    outcome: OutcomeKind,
    #[serde(default)]
    message: String,
    #[serde(default)]
    frames_run: u32,
    #[serde(default)]
    library_name: String,
    #[serde(default)]
    library_version: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    need_fullpath: bool,
    #[serde(default)]
    supports_no_game: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hw_render: Option<HwFile>,
    #[serde(default)]
    descriptors: Vec<DescriptorFile>,
}

const CONTEXT_NAMES: [(&str, HwContextKind); 7] = [
    ("opengl", HwContextKind::OpenGl),
    ("opengles2", HwContextKind::OpenGlEs2),
    ("opengl_core", HwContextKind::OpenGlCore),
    ("opengles3", HwContextKind::OpenGlEs3),
    ("opengles_version", HwContextKind::OpenGlEsVersion),
    ("vulkan", HwContextKind::Vulkan),
    ("direct3d11", HwContextKind::Direct3D11),
];

impl HwFile {
    fn from_request(hw: &HwRenderRequest) -> Self {
        let (context, raw) = match hw.context {
            HwContextKind::Unknown(raw) => ("unknown", raw),
            kind => (
                CONTEXT_NAMES
                    .iter()
                    .find(|(_, k)| *k == kind)
                    .map_or("unknown", |(name, _)| *name),
                0,
            ),
        };
        Self {
            context: context.to_owned(),
            raw,
            version_major: hw.version_major,
            version_minor: hw.version_minor,
        }
    }

    fn to_request(&self) -> HwRenderRequest {
        let context = CONTEXT_NAMES
            .iter()
            .find(|(name, _)| *name == self.context)
            .map_or(HwContextKind::Unknown(self.raw), |(_, kind)| *kind);
        HwRenderRequest::new(context, self.version_major, self.version_minor)
    }
}

impl ResultFile {
    fn from_report(report: &DryRunReport) -> Self {
        let (outcome, message) = match &report.outcome {
            DryRunOutcome::Probed => (OutcomeKind::Probed, String::new()),
            DryRunOutcome::Ran => (OutcomeKind::Ran, String::new()),
            DryRunOutcome::HardwareRenderRequired => {
                (OutcomeKind::HardwareRenderRequired, String::new())
            }
            DryRunOutcome::LoadFailed(message) => (OutcomeKind::LoadFailed, message.clone()),
            other => (OutcomeKind::LoadFailed, format!("{other:?}")),
        };
        Self {
            outcome,
            message,
            frames_run: report.frames_run,
            library_name: report.library_name.clone(),
            library_version: report.library_version.clone(),
            extensions: report.extensions.clone(),
            need_fullpath: report.need_fullpath,
            supports_no_game: report.supports_no_game,
            hw_render: report.hw_render.as_ref().map(HwFile::from_request),
            descriptors: report
                .descriptors
                .iter()
                .map(|d| DescriptorFile {
                    port: d.port,
                    device: d.device,
                    index: d.index,
                    id: d.id,
                    description: d.description.clone(),
                })
                .collect(),
        }
    }

    /// The report, bounded again: the file is the work of a child that runs foreign code.
    fn into_report(self) -> DryRunReport {
        let hw_render = self.hw_render.as_ref().map(HwFile::to_request);
        let outcome = match self.outcome {
            OutcomeKind::Probed => DryRunOutcome::Probed,
            OutcomeKind::Ran => DryRunOutcome::Ran,
            OutcomeKind::LoadFailed => {
                DryRunOutcome::LoadFailed(clean_text(&self.message, MAX_MESSAGE_CHARS))
            }
            OutcomeKind::HardwareRenderRequired => DryRunOutcome::HardwareRenderRequired,
        };
        DryRunReport {
            outcome,
            frames_run: self.frames_run,
            library_name: clean_text(&self.library_name, MAX_NAME_CHARS),
            library_version: clean_text(&self.library_version, MAX_NAME_CHARS),
            extensions: self
                .extensions
                .iter()
                .take(MAX_EXTENSIONS)
                .map(|e| clean_text(e, MAX_EXTENSION_CHARS))
                .filter(|e| !e.is_empty())
                .collect(),
            need_fullpath: self.need_fullpath,
            supports_no_game: self.supports_no_game,
            hw_render,
            descriptors: self
                .descriptors
                .into_iter()
                .filter_map(|d| {
                    let description = clean_text(&d.description, MAX_DESCRIPTION_CHARS);
                    (!description.is_empty()).then_some(InputDescriptor {
                        port: d.port,
                        device: d.device,
                        index: d.index,
                        id: d.id,
                        description,
                    })
                })
                .take(MAX_INPUT_DESCRIPTORS)
                .collect(),
        }
    }
}

/// `text` without control and bidirectional characters, at most `max` characters.
fn clean_text(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !crate::ui::text::is_format_char(*c))
        .take(max)
        .collect()
}

/// The whole file as text, `None` when it is missing, not a regular file, bigger than `MAX_FILE_BYTES`, or not
/// UTF-8.
fn read_bounded(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (u64::try_from(text.len()).ok()? <= MAX_FILE_BYTES).then_some(text)
}

/// Writes `text` to `path` through a neighbour `.tmp` file, so a reader never sees half of it.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Parent

/// Runs the child with `request` and waits at most `timeout`. The request and result files live in
/// `request.scratch_dir` (made here when missing).
pub fn run_dry_run(
    command: &DryRunCommand,
    request: &DryRunRequest,
    timeout: Duration,
) -> DryRunVerdict {
    let text = match toml::to_string(&RequestFile::from_request(request)) {
        Ok(text) => text,
        Err(e) => return DryRunVerdict::SpawnFailed(format!("requête non sérialisable : {e}")),
    };
    let dir = &request.scratch_dir;
    let request_file = dir.join(REQUEST_FILE);
    let result_file = dir.join(RESULT_FILE);
    let prepared = fs::create_dir_all(dir)
        .and_then(|()| match fs::remove_file(&result_file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        })
        .and_then(|()| write_atomic(&request_file, &text));
    if let Err(e) = prepared {
        return DryRunVerdict::SpawnFailed(format!("{} : {e}", dir.display()));
    }

    let mut child = match Command::new(&command.program)
        .args(&command.args)
        .arg(&request_file)
        .envs(command.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return DryRunVerdict::SpawnFailed(e.to_string()),
    };
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return DryRunVerdict::TimedOut;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return DryRunVerdict::SpawnFailed(e.to_string());
            }
        }
    };
    if !status.success() {
        return DryRunVerdict::Crashed {
            code: status.code(),
        };
    }
    match read_bounded(&result_file).and_then(|t| toml::from_str::<ResultFile>(&t).ok()) {
        Some(file) => verdict_of(file.into_report()),
        None => DryRunVerdict::Crashed { code: Some(0) },
    }
}

fn verdict_of(report: DryRunReport) -> DryRunVerdict {
    match &report.outcome {
        DryRunOutcome::Probed | DryRunOutcome::Ran => DryRunVerdict::Passed(report),
        DryRunOutcome::LoadFailed(message) => DryRunVerdict::LoadFailed(message.clone()),
        DryRunOutcome::HardwareRenderRequired => DryRunVerdict::HardwareRender(
            report
                .hw_render
                .unwrap_or_else(|| HwRenderRequest::new(HwContextKind::Unknown(0), 0, 0)),
        ),
        _ => DryRunVerdict::LoadFailed("résultat inconnu".to_owned()),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Child

/// Child side, called by `emu-app` before any other initialisation: bounded I/O, watchdog, no dialog, no journal.
/// Returns the process exit code; the result is the file next to the request.
pub fn dry_run_child_main(request_file: &Path) -> i32 {
    emu_runtime::suppress_crash_dialogs();
    let Some(request) =
        read_bounded(request_file).and_then(|text| toml::from_str::<RequestFile>(&text).ok())
    else {
        return EXIT_BAD_REQUEST;
    };
    let watchdog = Duration::from_secs(request.watchdog_secs.clamp(1, WATCHDOG_SECS));
    // The thread is never joined: it ends the process when the core is stuck, whatever happened to the parent.
    std::thread::spawn(move || {
        std::thread::sleep(watchdog);
        emu_runtime::exit_immediately(EXIT_WATCHDOG);
    });
    let report = emu_runtime::dry_run(&request.to_request());
    let written = toml::to_string(&ResultFile::from_report(&report))
        .map_err(io::Error::other)
        .and_then(|text| write_atomic(&request_file.with_file_name(RESULT_FILE), &text));
    if written.is_ok() {
        EXIT_OK
    } else {
        EXIT_WRITE_FAILED
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Dry-run folder

/// `<data>/cache/dry-run/<pid>-<n>/`: the request and result files, the copy of the game, and everything the core
/// and the child write (ADR 0025 §5: a dry run never touches the user's data). Created exclusively (`0700` on
/// Unix) with a locked `.lock` file, held while this value lives; removed on drop. Folders whose lock is free
/// belong to a dead instance: [`clean_stale_dry_runs`].
#[derive(Debug)]
pub struct DryRunDir {
    path: PathBuf,
    lock: Option<File>,
}

/// Creates `path` (it must not exist), private to the user on Unix.
fn create_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::DirBuilder::new().create(path)
    }
}

/// A dry-run folder younger than this is left alone by the cleanup (its owner may be starting up).
const STALE_MIN_AGE: Duration = Duration::from_secs(600);

static DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

fn dry_run_root(cache: &Path) -> PathBuf {
    cache.join("dry-run")
}

impl DryRunDir {
    pub fn create(cache: &Path) -> io::Result<Self> {
        let root = dry_run_root(cache);
        fs::create_dir_all(&root)?;
        let name = format!(
            "{}-{}",
            std::process::id(),
            DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = root.join(name);
        create_private_dir(&path)?;
        let made = File::create_new(path.join(LOCK_FILE)).and_then(|file| {
            file.try_lock().map_err(io::Error::other)?;
            Ok(file)
        });
        match made {
            Ok(lock) => Ok(Self {
                path,
                lock: Some(lock),
            }),
            Err(e) => {
                let _ = fs::remove_dir_all(&path);
                Err(e)
            }
        }
    }

    /// The folder: `DryRunRequest::scratch_dir`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A folder for private copies of a resident core (`DryRunRequest::copy_dir`), strictly inside the folder.
    pub fn copy_dir(&self) -> PathBuf {
        self.path.join(COPY_DIR)
    }

    /// Copies `game` (one regular file of at most [`MAX_GAME_COPY_BYTES`]) into the folder and returns the copy:
    /// the child can never change the original.
    pub fn copy_game(&self, game: &Path) -> Result<PathBuf, GameCopyError> {
        self.copy_game_limited(game, MAX_GAME_COPY_BYTES)
    }

    fn copy_game_limited(&self, game: &Path, limit: u64) -> Result<PathBuf, GameCopyError> {
        let meta = fs::metadata(game).map_err(GameCopyError::Io)?;
        if !meta.is_file() {
            return Err(GameCopyError::NotAFile);
        }
        if meta.len() > limit {
            return Err(GameCopyError::TooBig(meta.len()));
        }
        let name = game
            .file_name()
            .filter(|n| {
                Path::new(n)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)))
            })
            .ok_or(GameCopyError::NotAFile)?;
        let dir = self.path.join(GAME_DIR);
        fs::create_dir_all(&dir).map_err(GameCopyError::Io)?;
        let target = dir.join(name);
        // The size checked above can grow before the copy: read one byte past the limit and refuse.
        let copied = fs::File::open(game)
            .and_then(|src| {
                let mut out = File::create(&target)?;
                io::copy(&mut src.take(limit + 1), &mut out)
            })
            .map_err(GameCopyError::Io);
        let copied = match copied {
            Ok(n) if n > limit => Err(GameCopyError::TooBig(n)),
            other => other,
        };
        if let Err(e) = copied {
            let _ = fs::remove_file(&target);
            return Err(e);
        }
        Ok(target)
    }
}

impl Drop for DryRunDir {
    fn drop(&mut self) {
        // Remove while the lock is held, then let it go: no instance sees a free lock on a live folder.
        // On FAT/exFAT or with files still open (Windows) the removal can fail: `clean_stale_dry_runs` retries
        // at the next start, once the lock is free.
        let _ = fs::remove_dir_all(&self.path);
        self.lock = None;
    }
}

/// Why a game was not copied for the dry run (no dry run, only the « non testé » warning).
#[derive(Debug)]
pub enum GameCopyError {
    /// More than [`MAX_GAME_COPY_BYTES`] (the size).
    TooBig(u64),
    /// A folder, or a path without a plain file name.
    NotAFile,
    Io(io::Error),
}

/// `<pid>-<n>` made of digits.
fn is_dry_run_name(name: &str) -> bool {
    name.split_once('-').is_some_and(|(pid, n)| {
        [pid, n]
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Removes the dry-run folders of dead instances: those whose lock is free. A folder without a lock file is
/// removed only when it is more than an hour old (another instance may be creating it). A folder younger than
/// `STALE_MIN_AGE` is never touched. Never fatal.
pub fn clean_stale_dry_runs(cache: &Path) {
    clean_stale_older_than(cache, STALE_MIN_AGE);
}

fn clean_stale_older_than(cache: &Path, min_age: Duration) {
    let Ok(entries) = fs::read_dir(dry_run_root(cache)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_str().is_some_and(is_dry_run_name)
            || !entry.file_type().is_ok_and(|t| t.is_dir())
        {
            continue;
        }
        let path = entry.path();
        let age = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok());
        if age.is_some_and(|age| age < min_age) {
            continue;
        }
        let stale = match File::open(path.join(LOCK_FILE)) {
            Ok(lock) => lock.try_lock().is_ok(),
            Err(_) => entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t.elapsed().is_ok_and(|age| age > Duration::from_secs(3600))),
        };
        if stale && let Err(e) = fs::remove_dir_all(&path) {
            tracing::warn!("dossier d'essai {} non retiré : {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn request_round_trips_through_the_file() {
        let request = DryRunRequest {
            core: PathBuf::from("core.so"),
            content: Some(PathBuf::from("game.bin")),
            system_dir: PathBuf::from("system"),
            scratch_dir: PathBuf::from("scratch"),
            copy_dir: None,
            options: vec![("a".into(), "b \"c\"\n".into())],
            frames: 300,
        };
        let text = toml::to_string(&RequestFile::from_request(&request)).unwrap();
        let back: RequestFile = toml::from_str(&text).unwrap();
        assert_eq!(back.to_request(), request);
        assert_eq!(back.watchdog_secs, WATCHDOG_SECS);
    }

    #[test]
    fn request_with_an_unknown_key_is_refused() {
        let text = "core = 'c'\nsystem_dir = 's'\nscratch_dir = 'x'\nframes = 1\nextra = 1\n";
        assert!(toml::from_str::<RequestFile>(text).is_err());
    }

    #[test]
    fn frames_of_a_request_are_capped() {
        let text = "core = 'c'\nsystem_dir = 's'\nscratch_dir = 'x'\nframes = 4000000000\n";
        let file: RequestFile = toml::from_str(text).unwrap();
        assert_eq!(file.to_request().frames, MAX_FRAMES);
    }

    #[test]
    fn every_hardware_kind_round_trips() {
        let kinds = [
            HwContextKind::OpenGl,
            HwContextKind::OpenGlEs2,
            HwContextKind::OpenGlCore,
            HwContextKind::OpenGlEs3,
            HwContextKind::OpenGlEsVersion,
            HwContextKind::Vulkan,
            HwContextKind::Direct3D11,
            HwContextKind::Unknown(77),
        ];
        for kind in kinds {
            let hw = HwRenderRequest::new(kind, 4, 5);
            let text = toml::to_string(&HwFile::from_request(&hw)).unwrap();
            let back: HwFile = toml::from_str(&text).unwrap();
            assert_eq!(back.to_request(), hw, "{kind:?}");
        }
    }

    fn report(outcome: DryRunOutcome) -> DryRunReport {
        DryRunReport {
            outcome,
            frames_run: 12,
            library_name: "Core".into(),
            library_version: "1.0".into(),
            extensions: vec!["a".into(), "b".into()],
            need_fullpath: true,
            supports_no_game: false,
            hw_render: Some(HwRenderRequest::new(HwContextKind::Vulkan, 1, 3)),
            descriptors: vec![InputDescriptor {
                port: 1,
                device: 1,
                index: 0,
                id: 4,
                description: "Fire".into(),
            }],
        }
    }

    #[test]
    fn every_outcome_round_trips_through_the_result_file() {
        for outcome in [
            DryRunOutcome::Probed,
            DryRunOutcome::Ran,
            DryRunOutcome::LoadFailed("oups \"x\"\nligne".into()),
            DryRunOutcome::HardwareRenderRequired,
        ] {
            let expected = report(outcome);
            let text = toml::to_string(&ResultFile::from_report(&expected)).unwrap();
            let back: ResultFile = toml::from_str(&text).unwrap();
            let mut got = back.into_report();
            // Control characters of a message are removed by the parent.
            if let DryRunOutcome::LoadFailed(m) = &expected.outcome {
                assert_eq!(got.outcome, DryRunOutcome::LoadFailed(clean_text(m, 999)));
                got.outcome = expected.outcome.clone();
            }
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn the_parent_bounds_what_the_child_wrote() {
        let mut big = report(DryRunOutcome::Ran);
        big.library_name = "N\u{202e}ame".repeat(100);
        big.extensions = (0..500).map(|i| format!("e{i}")).collect();
        big.descriptors = (0..1000)
            .map(|i| InputDescriptor {
                port: 0,
                device: 1,
                index: 0,
                id: i % 16,
                description: format!("{}\u{7}{i}", "d".repeat(200)),
            })
            .collect();
        big.descriptors.push(InputDescriptor {
            port: 0,
            device: 1,
            index: 0,
            id: 0,
            description: "\u{7}".into(),
        });
        let got = ResultFile::from_report(&big).into_report();
        assert_eq!(got.library_name.chars().count(), MAX_NAME_CHARS);
        assert!(!got.library_name.contains('\u{202e}'));
        assert_eq!(got.extensions.len(), MAX_EXTENSIONS);
        assert_eq!(got.descriptors.len(), MAX_INPUT_DESCRIPTORS);
        assert!(
            got.descriptors
                .iter()
                .all(|d| d.description.chars().count() <= MAX_DESCRIPTION_CHARS
                    && !d.description.contains('\u{7}'))
        );
    }

    #[test]
    fn hardware_verdict_without_a_request_still_names_a_request() {
        let mut r = report(DryRunOutcome::HardwareRenderRequired);
        r.hw_render = None;
        assert!(matches!(verdict_of(r), DryRunVerdict::HardwareRender(_)));
    }

    #[test]
    fn read_bounded_refuses_big_missing_and_non_regular_files() {
        let d = dir();
        let big = d.path().join("big");
        fs::write(
            &big,
            vec![b'a'; usize::try_from(MAX_FILE_BYTES).unwrap() + 1],
        )
        .unwrap();
        assert!(read_bounded(&big).is_none());
        assert!(read_bounded(&d.path().join("missing")).is_none());
        assert!(read_bounded(d.path()).is_none());
        let ok = d.path().join("ok");
        fs::write(&ok, "x").unwrap();
        assert_eq!(read_bounded(&ok).as_deref(), Some("x"));
    }

    #[test]
    fn bad_request_file_is_an_exit_code_not_a_panic() {
        let d = dir();
        assert_eq!(
            dry_run_child_main(&d.path().join("missing.toml")),
            EXIT_BAD_REQUEST
        );
        let garbage = d.path().join("request.toml");
        fs::write(&garbage, "not = [toml").unwrap();
        assert_eq!(dry_run_child_main(&garbage), EXIT_BAD_REQUEST);
    }

    #[test]
    fn spawn_failure_is_a_verdict() {
        let d = dir();
        let command = DryRunCommand {
            program: d.path().join("no-such-program"),
            args: Vec::new(),
            env: Vec::new(),
        };
        let request = DryRunRequest {
            core: PathBuf::from("c"),
            content: None,
            system_dir: PathBuf::from("s"),
            scratch_dir: d.path().join("scratch"),
            copy_dir: None,
            options: Vec::new(),
            frames: 1,
        };
        assert!(matches!(
            run_dry_run(&command, &request, Duration::from_secs(1)),
            DryRunVerdict::SpawnFailed(_)
        ));
    }

    #[test]
    fn dry_run_dir_is_exclusive_private_and_removed_on_drop() {
        let cache = dir();
        let a = DryRunDir::create(cache.path()).unwrap();
        let b = DryRunDir::create(cache.path()).unwrap();
        assert_ne!(a.path(), b.path());
        assert!(a.path().starts_with(cache.path().join("dry-run")));
        assert!(is_dry_run_name(
            a.path().file_name().unwrap().to_str().unwrap()
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(a.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        assert!(a.copy_dir().starts_with(a.path()));
        let path = a.path().to_owned();
        drop(a);
        assert!(!path.exists());
        assert!(b.path().exists());
    }

    #[test]
    fn clean_stale_keeps_live_folders_and_removes_dead_ones() {
        let cache = dir();
        let live = DryRunDir::create(cache.path()).unwrap();
        // A dead instance: a folder with a free lock.
        let dead = cache.path().join("dry-run").join("999999-0");
        fs::create_dir_all(dead.join("game")).unwrap();
        fs::write(dead.join(LOCK_FILE), b"").unwrap();
        // Not ours: wrong name, and a file.
        let foreign = cache.path().join("dry-run").join("notes");
        fs::create_dir_all(&foreign).unwrap();
        let file = cache.path().join("dry-run").join("12-3");
        fs::write(&file, b"x").unwrap();
        // A fresh folder without a lock file (being created elsewhere) is kept.
        let creating = cache.path().join("dry-run").join("999998-0");
        fs::create_dir_all(&creating).unwrap();

        // Recent folders are left alone whatever their lock.
        clean_stale_dry_runs(cache.path());
        assert!(dead.exists(), "a recent dead folder is kept");

        clean_stale_older_than(cache.path(), Duration::ZERO);

        assert!(!dead.exists());
        assert!(live.path().exists());
        assert!(foreign.exists());
        assert!(file.exists());
        assert!(creating.exists());
    }

    #[test]
    fn clean_stale_without_a_cache_is_a_no_op() {
        let cache = dir();
        clean_stale_dry_runs(&cache.path().join("absent"));
    }

    #[test]
    fn game_copy_is_a_separate_file_within_the_limit() {
        let cache = dir();
        let dry = DryRunDir::create(cache.path()).unwrap();
        let game = cache.path().join("g.bin");
        fs::write(&game, b"abcdef").unwrap();
        let copy = dry.copy_game(&game).unwrap();
        assert!(copy.starts_with(dry.path()));
        assert_eq!(copy.file_name(), game.file_name());
        fs::write(&copy, b"changed").unwrap();
        assert_eq!(fs::read(&game).unwrap(), b"abcdef");
        assert!(matches!(
            dry.copy_game_limited(&game, 5),
            Err(GameCopyError::TooBig(6))
        ));
        assert!(dry.copy_game_limited(&game, 6).is_ok());
        assert!(matches!(
            dry.copy_game(cache.path()),
            Err(GameCopyError::NotAFile)
        ));
        assert!(matches!(
            dry.copy_game(&cache.path().join("missing")),
            Err(GameCopyError::Io(_))
        ));
    }
}
