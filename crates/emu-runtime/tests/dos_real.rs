//! Real DOS tests (4.0): dosbox_pure headless on generated or free programs only (`docs/design/v4.0-dos.md` §6).
//! No commercial software anywhere: `tests/tools/mkdos.py` programs, FreeDOS 1.4 and Floppy Bird.
//!
//! All tests are `#[ignore]`: run `scripts/fetch-test-assets.sh --with-4.0` then
//! `cargo test -p emu-runtime --test dos_real -- --ignored --test-threads=1`.
//!
//! **One process per emulation session, for isolation.** dosbox_pure stays mapped after `dlclose` (libstdc++ binds to
//! its exported templates), so its static state survives `retro_deinit`; since ADR 0022 `emu-libretro` reloads it
//! from a private copy of the library, which works in the same process (see
//! `two_dos_games_in_a_row_in_the_same_process`). The other tests still drive each session through `play`, which
//! re-executes this test binary (`zz_worker`) with the scenario in `DOS_PLAY`: every session then starts from a
//! pristine process, so the pinned fingerprints do not depend on what ran before. The worker reads back
//! `@@ key=value` lines. Set `EMU_UPDATE_FINGERPRINTS=1` to
//! (re)write `tests/fingerprints/dos-dosbox-pure.txt` after inspecting the PNGs in `target/test-output/dos/`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use emu_runtime::{
    CollectSink, ContentSpec, Emulator, EmulatorOptions, MAX_COPY_BYTES, MediaPolicy, MediaRules,
    MediaSet, RetroKey, ScriptedInput, core_id_from_path, snapshot_path,
};
use sha2::{Digest, Sha256};

/// One libretro core per process (ADR 0008): tests must not overlap.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn core_path() -> PathBuf {
    root().join("tests/cores/dosbox_pure_libretro.so")
}

fn roms() -> PathBuf {
    root().join("tests/roms/dos")
}

fn out_dir(name: &str) -> PathBuf {
    let dir = root().join("target/test-output/dos").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create output dir");
    dir
}

fn require(path: &Path) -> &Path {
    assert!(
        path.is_file(),
        "missing test asset {}: lancer scripts/fetch-test-assets.sh --with-4.0",
        path.display()
    );
    path
}

/// Writes every `mkdos.py` case under `out` (fixed zip dates).
fn mkdos(out: &Path) {
    let run = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools/mkdos.py"))
        .arg(out)
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .output()
        .expect("python3 is required to run mkdos.py");
    assert!(
        run.status.success(),
        "mkdos.py failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

/// SHA-256 of a file, or of a whole tree (relative names, bytes and folders) for a directory.
fn digest(path: &Path) -> String {
    fn walk(root: &Path, dir: &Path, h: &mut Sha256) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            h.update(rel.as_bytes());
            if p.is_dir() {
                h.update(b"/");
                walk(root, &p, h);
            } else {
                h.update(fs::read(&p).unwrap());
            }
        }
    }
    let mut h = Sha256::new();
    if path.is_dir() {
        walk(path, path, &mut h);
    } else {
        h.update(fs::read(path).unwrap());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Fresh data directories under `data` (its path may contain `#`: see `start_exe_with_a_hash_in_the_cache_path`).
fn options(data: &Path, provide_vfs: bool) -> EmulatorOptions {
    for sub in ["system", "saves", "states", "assets"] {
        fs::create_dir_all(data.join(sub)).unwrap();
    }
    EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        cache_dir: data.join("cache"),
        provide_vfs,
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: Vec::new(),
        restore_added_disks: false,
        core_save_scope: emu_runtime::SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
    }
}

/// The rules of `cores.toml` for dosbox_pure.
fn rules() -> MediaRules {
    MediaRules {
        policy: MediaPolicy::WorkingCopy,
        media_direct: ["zip", "dosz", "iso", "cue", "chd", "ins"]
            .map(String::from)
            .to_vec(),
        max_copy_bytes: MAX_COPY_BYTES,
        m3u_launch: emu_runtime::M3uLaunch::None,
    }
}

fn image(name: &str, path: &Path) -> ContentSpec {
    ContentSpec::Media {
        set: MediaSet {
            name: name.into(),
            images: vec![path.to_path_buf()],
            playlist: None,
            launch: None,
        },
        rules: rules(),
    }
}

fn folder(dir: &Path, start: Option<&str>) -> ContentSpec {
    ContentSpec::Folder {
        dir: dir.to_path_buf(),
        name: dir.file_name().unwrap().to_string_lossy().into_owned(),
        start: start.map(str::to_owned),
    }
}

// ---- fingerprints (same file format as `amiga_puae.rs`) ------------------------------------------------------

const PRINTS: &str = "tests/fingerprints/dos-dosbox-pure.txt";

fn core_hash() -> String {
    fs::read_to_string(root().join("tests/cores/dosbox_pure_libretro.so.sha256"))
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// `name frame=N fingerprint=H` lines.
fn parse_prints(text: &str) -> Vec<(String, u64, u64)> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut parts = l.split_whitespace();
            let name = parts.next().unwrap().to_owned();
            let (mut frame, mut fp) = (None, None);
            for part in parts {
                if let Some(v) = part.strip_prefix("frame=") {
                    frame = v.parse().ok();
                } else if let Some(v) = part.strip_prefix("fingerprint=") {
                    fp = u64::from_str_radix(v, 16).ok();
                }
            }
            (
                name,
                frame.unwrap_or_else(|| panic!("bad fingerprint line: {l}")),
                fp.unwrap_or_else(|| panic!("bad fingerprint line: {l}")),
            )
        })
        .collect()
}

/// One file for the whole binary, filled by every test; compared (or, with `EMU_UPDATE_FINGERPRINTS`, merged)
/// by name so that tests may run alone.
fn check_fingerprint(name: &str, frame: u64, fp: u64) {
    let path = root().join(PRINTS);
    let mut known = fs::read_to_string(&path)
        .map(|t| parse_prints(&t))
        .unwrap_or_default();
    if std::env::var_os("EMU_UPDATE_FINGERPRINTS").is_some() {
        known.retain(|(n, _, _)| n != name);
        known.push((name.to_owned(), frame, fp));
        known.sort();
        let mut out = format!(
            "# dosbox_pure, generated or free DOS programs, scripted keys only; core sha256 {}\n",
            core_hash()
        );
        for (n, f, p) in &known {
            out.push_str(&format!("{n} frame={f} fingerprint={p:016x}\n"));
        }
        fs::write(&path, out).unwrap();
        return;
    }
    let want = known
        .iter()
        .find(|(n, _, _)| n == name)
        .unwrap_or_else(|| {
            panic!(
                "no fingerprint `{name}` in {}: inspect target/test-output/dos/*/*.png then run with EMU_UPDATE_FINGERPRINTS=1",
                path.display()
            )
        });
    assert_eq!(
        (want.1, want.2),
        (frame, fp),
        "fingerprint `{name}` differs; regenerate with EMU_UPDATE_FINGERPRINTS=1 if the change is legitimate"
    );
}

// ---- one process per session ----------------------------------------------------------------------------------

type Params = BTreeMap<String, String>;

/// `DOS_PLAY` is `key=value` lines.
fn parse_params(text: &str) -> Params {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn get<'a>(p: &'a Params, key: &str) -> Option<&'a str> {
    p.get(key).map(String::as_str).filter(|v| !v.is_empty())
}

fn num<T: std::str::FromStr>(p: &Params, key: &str, default: T) -> T {
    get(p, key).map_or(default, |v| {
        v.parse().unwrap_or_else(|_| panic!("bad `{key}`: {v}"))
    })
}

/// Input script: `key:<retro key>:<from>:<to>` and `mouse:<dx>:<dy>:<buttons>:<from>:<to>` separated by `|`.
fn script(spec: &str) -> ScriptedInput {
    let mut input = ScriptedInput::new();
    for item in spec.split('|').filter(|i| !i.is_empty()) {
        let f: Vec<&str> = item.split(':').collect();
        input = match f.as_slice() {
            ["key", key, from, to] => input.key(
                RetroKey::new(key.parse().unwrap()).expect("retro key"),
                from.parse().unwrap()..to.parse().unwrap(),
            ),
            ["mouse", dx, dy, buttons, from, to] => input.mouse(
                dx.parse().unwrap(),
                dy.parse().unwrap(),
                buttons.parse().unwrap(),
                from.parse().unwrap()..to.parse().unwrap(),
            ),
            _ => panic!("bad script item `{item}`"),
        };
    }
    input
}

/// Worker side: one session, then the report.
fn session(p: &Params) -> BTreeMap<String, String> {
    let data = PathBuf::from(get(p, "data").expect("data"));
    let content = get(p, "content").expect("content");
    let (kind, path) = content.split_once(':').expect("content kind:path");
    let path = PathBuf::from(path);
    let start = get(p, "start");
    let spec = match kind {
        "file" => ContentSpec::File(path),
        "folder" => folder(&path, start),
        "image" => image(&path.file_stem().unwrap().to_string_lossy(), &path),
        _ => panic!("unknown content kind {kind}"),
    };
    require(&core_path());
    // The core may use libm without linking it itself: the host process must have it loaded (emu-app does,
    // through egui), as for puae.
    assert!(std::hint::black_box(0.5f32).sin() > 0.0);
    for name in get(p, "system_files")
        .unwrap_or("")
        .split(',')
        .filter(|n| !n.is_empty())
    {
        fs::create_dir_all(data.join("system")).unwrap();
        fs::write(data.join("system").join(name), b"not a real SoundFont").unwrap();
    }
    let started = Instant::now();
    let mut emu = Emulator::load_content(&core_path(), spec, options(&data, num(p, "vfs", 1) == 1))
        .expect("load dosbox_pure");
    let load_ms = started.elapsed().as_millis();

    let mut input = script(get(p, "script").unwrap_or(""));
    let mut audio = CollectSink::default();
    let frames: u64 = num(p, "frames", 600);
    let mut report = BTreeMap::new();
    let mut first_frame_ms = 0;
    let mut run = |emu: &mut Emulator, n: u64, audio: &mut CollectSink, first: &mut u128| {
        let t = Instant::now();
        for i in 0..n {
            emu.run_frame(&mut input, audio).expect("run_frame");
            if *first == 0 && i == 0 {
                *first = started.elapsed().as_millis();
            }
        }
        t.elapsed().as_millis()
    };
    let run_ms = run(&mut emu, frames, &mut audio, &mut first_frame_ms);
    let png_path = get(p, "png").map(PathBuf::from);
    let shot = |emu: &Emulator, suffix: &str| {
        if let Some(png) = &png_path {
            let name = format!("{}{suffix}.png", png.file_stem().unwrap().to_string_lossy());
            emu.frame().write_png(&png.with_file_name(name)).unwrap();
        }
    };
    shot(&emu, "");
    let mut put = |k: &str, v: String| {
        report.insert(k.to_owned(), v);
    };
    put("fp", format!("{:016x}", emu.frame().fingerprint()));
    // Reset scenario: `reset=shift` (reset_with_keys(LSHIFT)) or `reset=plain` (Core::reset), then `after` frames.
    if let Some(kind) = get(p, "reset") {
        put("fp_before", format!("{:016x}", emu.frame().fingerprint()));
        match kind {
            "shift" => emu.reset_with_keys(&[RetroKey::new(304).unwrap()]),
            "plain" => emu.reset(),
            other => panic!("unknown reset {other}"),
        }
        run(
            &mut emu,
            num(p, "after", 300),
            &mut audio,
            &mut first_frame_ms,
        );
        shot(&emu, "-after-reset");
        put("fp", format!("{:016x}", emu.frame().fingerprint()));
    }
    put("w", emu.frame().width().to_string());
    put("h", emu.frame().height().to_string());
    put("fps", emu.av_info().timing.fps.to_string());
    put("rate", emu.av_info().timing.sample_rate.to_string());
    put("samples", audio.samples.len().to_string());
    put(
        "peak",
        audio
            .samples
            .iter()
            .map(|s| i32::from(*s).abs())
            .max()
            .unwrap_or(0)
            .to_string(),
    );
    put("content_name", emu.content_name().unwrap_or("").to_owned());
    put(
        "save_dir",
        emu.identity().core_save_dir.display().to_string(),
    );
    put("keyboard", emu.uses_keyboard().to_string());
    put("mouse", emu.uses_mouse().to_string());
    put("load_ms", load_ms.to_string());
    put("first_frame_ms", first_frame_ms.to_string());
    put("run_ms", run_ms.to_string());
    if let Some(key) = get(p, "option") {
        let values = emu
            .options()
            .into_iter()
            .find(|o| o.key == key)
            .map(|o| o.values.into_iter().map(|v| v.value).collect::<Vec<_>>())
            .unwrap_or_default();
        put("option_values", values.join(","));
    }
    drop(emu);
    report
}

/// Worker entry point: does nothing unless `DOS_PLAY` is set (the parent re-executes this binary on it).
#[test]
#[ignore = "worker of the other tests (needs DOS_PLAY)"]
fn zz_worker() {
    let Some(text) = std::env::var_os("DOS_PLAY") else {
        return;
    };
    let report = session(&parse_params(&text.to_string_lossy()));
    println!();
    for (k, v) in report {
        println!("@@ {k}={v}");
    }
}

/// Worker of `two_dos_games_in_a_row_in_the_same_process`: three sessions (hello, menu, hello) in this process,
/// each a full `Emulator` life cycle in the same data directory (the application's "next game" case).
#[test]
#[ignore = "worker of the other tests (needs DOS_SEQUENCE)"]
fn zz_sequence_worker() {
    let Some(text) = std::env::var_os("DOS_SEQUENCE") else {
        return;
    };
    let mut lines = text
        .to_string_lossy()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let data = lines.remove(0);
    println!();
    for (i, content) in lines.iter().enumerate() {
        let params = parse_params(&format!("data={data}\ncontent={content}\nframes=600"));
        let report = session(&params);
        println!("@@ fp{i}={}", report["fp"]);
    }
}

/// Result of one session.
struct Played {
    report: BTreeMap<String, String>,
    /// Data directory (`<out>/<tag>/data` unless `data` was given).
    data: PathBuf,
}

impl Played {
    fn s(&self, key: &str) -> &str {
        self.report
            .get(key)
            .unwrap_or_else(|| panic!("no `{key}` in the report: {:?}", self.report))
    }

    fn n(&self, key: &str) -> u128 {
        self.s(key).parse().unwrap_or_else(|_| panic!("{key}"))
    }

    fn fp(&self) -> u64 {
        u64::from_str_radix(self.s("fp"), 16).unwrap()
    }

    fn cache(&self) -> PathBuf {
        self.data
            .join("cache")
            .join(core_id_from_path(&core_path()))
    }
}

/// Runs a session in a fresh process. `tag` names `target/test-output/dos/<tag>/` (data directory, PNG).
fn play(tag: &str, params: &[(&str, &str)]) -> Played {
    let out = out_dir(tag);
    let mut all: Params = params
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    let data = all
        .entry("data".into())
        .or_insert_with(|| out.join("data").display().to_string())
        .clone();
    all.entry("png".into())
        .or_insert_with(|| out.join(format!("{tag}.png")).display().to_string());
    let mut text = String::new();
    for (k, v) in &all {
        writeln!(text, "{k}={v}").unwrap();
    }
    let run = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "zz_worker",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DOS_PLAY", text)
        .output()
        .expect("spawn the worker");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "session `{tag}` failed ({}):\n{stdout}\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    let report = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("@@ "))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    Played {
        report,
        data: PathBuf::from(data),
    }
}

/// Two sessions (`<tag>-a`, `<tag>-b`, fresh data directories) must show the same last frame, which is then pinned.
fn play_twice_and_pin(tag: &str, params: &[(&str, &str)]) -> Played {
    let a = play(&format!("{tag}-a"), params);
    let b = play(&format!("{tag}-b"), params);
    assert_eq!(a.fp(), b.fp(), "{tag}: two sessions must be identical");
    let frames: u64 = params
        .iter()
        .find(|(k, _)| *k == "frames")
        .map_or(600, |(_, v)| v.parse().unwrap());
    let after: u64 = params
        .iter()
        .find(|(k, _)| *k == "after")
        .map_or(0, |(_, v)| v.parse().unwrap());
    check_fingerprint(tag, frames + after, a.fp());
    a
}

fn games(tag: &str) -> PathBuf {
    let dir = out_dir(tag).join("games");
    mkdos(&dir);
    dir
}

fn p(path: &Path) -> String {
    path.display().to_string()
}

// ---- tests ---------------------------------------------------------------------------------------------------

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn hello_zip_and_folder_show_the_text_and_leave_the_sources_alone() {
    let _guard = lock();
    let games = games("hello-sources");
    let (zip, dir) = (games.join("hello.zip"), games.join("hello"));
    let (zip_before, dir_before) = (digest(&zip), digest(&dir));

    let z = play_twice_and_pin(
        "hello-zip",
        &[("content", &format!("file:{}", zip.display()))],
    );
    assert!(
        z.s("keyboard") == "true",
        "the core registers a keyboard callback"
    );
    // Text mode 80x25, doubled.
    assert_eq!((z.s("w"), z.s("h")), ("640", "400"));
    let f = play_twice_and_pin(
        "hello-folder",
        &[("content", &format!("folder:{}", dir.display()))],
    );
    assert_eq!(f.s("content_name"), "hello");
    assert!(
        snapshot_path(&f.cache(), "hello").is_file(),
        "the folder reached the core as a snapshot in the cache"
    );
    assert_eq!(
        z.fp(),
        f.fp(),
        "the same program shows the same text from a zip and from a folder"
    );
    assert_eq!(digest(&zip), zip_before, "the zip must stay untouched");
    assert_eq!(digest(&dir), dir_before, "the folder must stay untouched");
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn two_dos_games_in_a_row_in_the_same_process() {
    let _guard = lock();
    let games = games("same-process");
    let hello = format!("file:{}", games.join("hello.zip").display());
    let menu = format!("folder:{}", games.join("menu").display());
    let data = out_dir("same-process-data");

    // Reference: hello in a pristine process.
    let reference = play("same-process-ref", &[("content", &hello)]).fp();

    // hello, menu, hello again, all in one process: no crash, and the third run equals a fresh process.
    let text = format!("{}\n{hello}\n{menu}\n{hello}", data.display());
    let run = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "zz_sequence_worker",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DOS_SEQUENCE", text)
        .output()
        .expect("spawn the worker");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "the sequence crashed ({}):\n{stdout}\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    let fp = |i: usize| {
        let key = format!("@@ fp{i}=");
        let hex = stdout
            .lines()
            .find_map(|l| l.strip_prefix(&key))
            .unwrap_or_else(|| panic!("no {key} in:\n{stdout}"));
        u64::from_str_radix(hex, 16).unwrap()
    };
    assert_eq!(fp(0), reference, "first session = fresh process");
    assert_ne!(fp(0), fp(1), "the second game shows something else");
    assert_eq!(
        fp(2),
        reference,
        "reload through the private copy = fresh process"
    );
    // The copies are unlinked as soon as they are mapped.
    let copies = data.join("cache/core-copies");
    assert_eq!(fs::read_dir(&copies).map_or(0, Iterator::count), 0);
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn start_exe_chooses_the_program_of_a_folder() {
    let _guard = lock();
    let games = games("start");
    let dir = games.join("menu");
    let before = digest(&dir);
    let content = format!("folder:{}", dir.display());

    // Without `start`: two executables, the core shows its start menu.
    let menu = play_twice_and_pin("menu-folder", &[("content", &content)]);
    // With `start` (any case): `<snapshot>.zip#MENU2.COM` runs that program directly.
    let two = play_twice_and_pin(
        "menu-folder-start-menu2",
        &[("content", &content), ("start", "MENU2.COM")],
    );
    let one = play_twice_and_pin(
        "menu-folder-start-menu1",
        &[("content", &content), ("start", "menu1.com")],
    );
    assert_ne!(menu.fp(), two.fp());
    assert_ne!(menu.fp(), one.fp());
    assert_ne!(one.fp(), two.fp(), "each start program prints its own text");
    assert_eq!(digest(&dir), before);
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn start_exe_works_with_a_hash_in_the_cache_path() {
    let _guard = lock();
    let games = games("starthash-games");
    let dir = games.join("menu");
    let content = format!("folder:{}", dir.display());
    let plain = play(
        "start-hash-plain",
        &[("content", &content), ("start", "MENU2.COM")],
    );
    // A `#` in the data directory (so in the cache path) must not be taken for the start-program separator.
    let hashed = out_dir("start-hash-dir").join("data #1 (test)");
    let with_hash = play(
        "start-hash",
        &[
            ("content", &content),
            ("start", "MENU2.COM"),
            ("data", &p(&hashed)),
        ],
    );
    assert!(
        snapshot_path(&with_hash.cache(), "menu").is_file(),
        "snapshot built under the `#` path"
    );
    assert!(with_hash.cache().to_string_lossy().contains('#'));
    assert_eq!(
        with_hash.fp(),
        plain.fp(),
        "same program as without `#` in the path"
    );
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn vfs_provided_lists_the_system_directory_and_not_provided_does_not() {
    let _guard = lock();
    let games = games("vfs");
    let content = format!("file:{}", games.join("hello.zip").display());
    let with = |tag: &str, vfs: &str| {
        play(
            tag,
            &[
                ("content", &content),
                ("vfs", vfs),
                ("frames", "30"),
                ("system_files", "Test.sf2,Other.SF2"),
                ("option", "dosbox_pure_midi"),
            ],
        )
    };
    let on = with("vfs-on", "1");
    let off = with("vfs-off", "0");
    let list = |r: &Played| {
        r.s("option_values")
            .split(',')
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    eprintln!("on {:?} off {:?}", list(&on), list(&off));
    assert!(
        list(&on).contains(&"Test.sf2".to_owned()),
        "{:?}",
        list(&on)
    );
    assert!(
        list(&on).contains(&"Other.SF2".to_owned()),
        "{:?}",
        list(&on)
    );
    assert!(
        !list(&off)
            .iter()
            .any(|v| v.to_lowercase().ends_with(".sf2")),
        "{:?}",
        list(&off)
    );
    // Still the core's two built-in choices either way.
    for r in [&on, &off] {
        assert!(
            list(r).contains(&"frontend".to_owned()) && list(r).contains(&"disabled".to_owned())
        );
    }
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn reset_with_shift_shows_the_start_menu_and_a_plain_reset_does_not() {
    let _guard = lock();
    let games = games("reset");
    let content = format!("file:{}", games.join("hello.zip").display());
    let params = |kind: &'static str| {
        [
            ("content", content.clone()),
            ("frames", "400".into()),
            ("reset", kind.into()),
            ("after", "400".into()),
        ]
    };
    let run = |tag: &str, kind: &'static str| {
        let p = params(kind);
        play(tag, &p.each_ref().map(|(k, v)| (*k, v.as_str())))
    };
    let shift = run("reset-shift", "shift");
    let plain = run("reset-plain", "plain");
    // Before the reset every run shows the program (auto-started: the zip holds one executable).
    assert_eq!(shift.s("fp_before"), plain.s("fp_before"));
    // A plain reset starts the same program again; Shift held during `retro_reset` forces the start menu.
    assert_ne!(
        shift.fp(),
        plain.fp(),
        "Shift must change what the core shows"
    );
    assert_ne!(shift.s("fp"), shift.s("fp_before"));
    check_fingerprint("reset-shift", 800, shift.fp());
    check_fingerprint("reset-plain", 800, plain.fp());
    // The plain reset shows what the program showed before it (same text, same frame).
    assert_eq!(plain.s("fp"), plain.s("fp_before"));
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn the_core_runs_at_70_hz_with_48_khz_audio() {
    let _guard = lock();
    let games = games("av");
    let r = play(
        "av",
        &[
            (
                "content",
                &format!("file:{}", games.join("hello.zip").display()),
            ),
            ("frames", "700"),
        ],
    );
    let fps: f64 = r.s("fps").parse().unwrap();
    let rate: f64 = r.s("rate").parse().unwrap();
    assert!((fps - 70.0866).abs() < 0.001, "{fps}");
    assert_eq!(rate, 48_000.0);
    // 700 frames of stereo audio at 48 kHz / 70.0866 Hz, within 2 %.
    let expected = 700.0 / fps * rate * 2.0;
    let got: f64 = r.s("samples").parse().unwrap();
    assert!(
        (got - expected).abs() / expected < 0.02,
        "{got} vs {expected}"
    );
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn freedos_boots_from_its_floppy_image_and_the_image_stays_intact() {
    let _guard = lock();
    let fd = roms().join("freedos-1.4-x86BOOT.img");
    require(&fd);
    let before = digest(&fd);
    // Direct `.img`: the core's menu offers « Boot from Disk Image » one line above the pre-selected program;
    // Up, Enter, then Enter on the first video mode (SVGA) boots the installer's FreeCOM shell.
    let boot = [
        ("content", format!("image:{}", fd.display())),
        (
            "script",
            "key:273:100:105|key:13:130:135|key:13:200:205".to_owned(),
        ),
        ("frames", "3500".to_owned()),
    ];
    let args: Vec<(&str, &str)> = boot.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let menu = play(
        "freedos-menu",
        &[
            ("content", &format!("image:{}", fd.display())),
            ("frames", "600"),
        ],
    );
    let booted = play_twice_and_pin("freedos-boot", &args);
    assert_ne!(menu.fp(), booted.fp());
    check_fingerprint("freedos-menu", 600, menu.fp());
    assert_eq!(
        digest(&fd),
        before,
        "the original image must stay untouched"
    );
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn floppy_bird_runs_in_320x200() {
    let _guard = lock();
    let bird = roms().join("FLPYBIRD.COM");
    require(&bird);
    let before = digest(&bird);
    let r = play_twice_and_pin(
        "floppy-bird",
        &[
            ("content", &format!("file:{}", bird.display())),
            ("frames", "300"),
        ],
    );
    assert_eq!((r.s("w"), r.s("h")), ("320", "200"), "VGA mode 13h");
    // The refresh rate follows the video mode (70.086 Hz in text mode, 70.086 / 70.083 in 13h): always ~70 Hz.
    let fps: f64 = r.s("fps").parse().unwrap();
    assert!((fps - 70.0).abs() < 0.2, "{fps}");
    assert_eq!(digest(&bird), before);
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn sessions_that_write_leave_the_zip_and_the_folder_intact() {
    let _guard = lock();
    let games = games("writes");
    let (zip, dir) = (games.join("bat.zip"), games.join("bat"));
    let (zip_before, dir_before) = (digest(&zip), digest(&dir));

    // RUN.BAT writes OUT.TXT on C: : the core keeps the difference in `<saves>/dosbox_pure/<name>.pure.zip`.
    let z = play(
        "writes-zip",
        &[("content", &format!("file:{}", zip.display()))],
    );
    let pure = PathBuf::from(z.s("save_dir")).join("bat.pure.zip");
    assert!(pure.is_file(), "{} missing", pure.display());
    let f = play(
        "writes-folder",
        &[("content", &format!("folder:{}", dir.display()))],
    );
    let pure = PathBuf::from(f.s("save_dir")).join("bat.pure.zip");
    assert!(pure.is_file(), "{} missing", pure.display());
    // The snapshot in the cache is the zip the core read: it is not where the modifications go.
    let snapshot = snapshot_path(&f.cache(), "bat");
    let names = {
        let mut a = zip::ZipArchive::new(fs::File::open(&pure).unwrap()).unwrap();
        (0..a.len())
            .map(|i| String::from_utf8_lossy(a.by_index(i).unwrap().name_raw()).into_owned())
            .collect::<Vec<_>>()
    };
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("OUT.TXT")),
        "{names:?}"
    );
    assert!(snapshot.is_file());
    assert_eq!(
        digest(&zip),
        zip_before,
        "the user's zip must stay untouched"
    );
    assert_eq!(
        digest(&dir),
        dir_before,
        "the user's folder must stay untouched"
    );
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn generated_floppy_and_iso_are_read_and_the_originals_stay_intact() {
    let _guard = lock();
    let games = games("media");
    let (img, ima, iso) = (
        games.join("hello.img"),
        games.join("hello.ima"),
        games.join("hello.iso"),
    );
    let before = [digest(&img), digest(&ima), digest(&iso)];
    // The direct floppy shows the core's menu listing `A:\HELLO.COM` (read from our FAT12): Enter runs it.
    let menu = play(
        "generated-floppy-menu",
        &[("content", &format!("image:{}", img.display()))],
    );
    let run = |tag: &str, file: &Path| {
        play_twice_and_pin(
            tag,
            &[
                ("content", &format!("image:{}", file.display())),
                ("script", "key:13:100:105"),
                ("frames", "600"),
            ],
        )
    };
    let floppy = run("generated-floppy", &img);
    let ima_run = run("generated-floppy-ima", &ima);
    assert_ne!(menu.fp(), floppy.fp());
    assert_eq!(
        floppy.fp(),
        ima_run.fp(),
        ".img and .ima are the same floppy"
    );
    play_twice_and_pin(
        "generated-iso",
        &[("content", &format!("file:{}", iso.display()))],
    );
    assert_eq!([digest(&img), digest(&ima), digest(&iso)], before);
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn generated_keys_and_mouse_programs_react_to_the_input() {
    let _guard = lock();
    let games = games("input");
    let keys = format!("file:{}", games.join("keys.zip").display());
    let mouse = format!("file:{}", games.join("mouse.zip").display());
    // KEYS.COM echoes the keys read with INT 16h: `a` then `b` (US layout, the default), Esc would quit.
    let typed = play_twice_and_pin(
        "keys-ab",
        &[
            ("content", &keys),
            ("script", "key:97:200:210|key:98:230:240"),
            ("frames", "400"),
        ],
    );
    let idle = play("keys-idle", &[("content", &keys), ("frames", "400")]);
    assert_ne!(typed.fp(), idle.fp());
    // MOUSE.COM prints the INT 33h position: a relative host motion (+8 per frame for 100 frames) moves it.
    let moved = play_twice_and_pin(
        "mouse-moved",
        &[
            ("content", &mouse),
            ("script", "mouse:8:0:0:200:300"),
            ("frames", "400"),
        ],
    );
    let still = play("mouse-still", &[("content", &mouse), ("frames", "400")]);
    assert_ne!(moved.fp(), still.fp());
    assert_eq!(moved.s("mouse"), "true", "the core polled the mouse");
}

#[test]
#[ignore = "needs dosbox_pure and the DOS test data: scripts/fetch-test-assets.sh --with-4.0"]
fn unbuffered_vfs_io_does_not_slow_the_startup_down() {
    let _guard = lock();
    // A 23 MB zip read by the core: with `provide_vfs` its file I/O goes through our unbuffered VFS
    // (`read` / `seek` straight to the OS), without it through the core's own `fopen`.
    let zip = roms().join("FD14-FloppyEdition.zip");
    require(&zip);
    let content = format!("file:{}", zip.display());
    let measure = |vfs: &str| {
        let mut best = (u128::MAX, u128::MAX);
        for i in 0..3 {
            let r = play(
                &format!("perf-vfs{vfs}-{i}"),
                &[("content", &content), ("vfs", vfs), ("frames", "60")],
            );
            best = (
                best.0.min(r.n("load_ms")),
                best.1.min(r.n("first_frame_ms")),
            );
        }
        best
    };
    let (load_on, first_on) = measure("1");
    let (load_off, first_off) = measure("0");
    eprintln!(
        "startup (best of 3): vfs load {load_on} ms, first frame {first_on} ms; no vfs load {load_off} ms, first frame {first_off} ms"
    );
    // Generous: catches a pathological per-byte syscall pattern, not noise.
    assert!(
        first_on <= first_off * 3 + 2000,
        "first frame {first_on} ms with the VFS vs {first_off} ms without"
    );
    assert!(first_on < 20_000, "first frame after {first_on} ms");

    // Sustained reads: FreeDOS booting from its floppy (the working copy is read sector by sector).
    let fd = roms().join("freedos-1.4-x86BOOT.img");
    require(&fd);
    let content = format!("image:{}", fd.display());
    let boot = |vfs: &str| {
        (0..2)
            .map(|i| {
                play(
                    &format!("perf-boot-vfs{vfs}-{i}"),
                    &[
                        ("content", &content),
                        ("vfs", vfs),
                        ("script", "key:273:100:105|key:13:130:135|key:13:200:205"),
                        ("frames", "1500"),
                    ],
                )
                .n("run_ms")
            })
            .min()
            .unwrap()
    };
    let (boot_on, boot_off) = (boot("1"), boot("0"));
    eprintln!(
        "1500 frames of the FreeDOS boot (best of 2): vfs {boot_on} ms, no vfs {boot_off} ms"
    );
    assert!(
        boot_on <= boot_off * 2 + 1000,
        "boot {boot_on} ms with the VFS vs {boot_off} ms without"
    );
}
