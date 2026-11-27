//! The dry-run child for real (ADR 0025 §5): `emu_ui::run_dry_run` starts the `emu-rust` binary with
//! `--dry-run-child`. The tests that need the mock core and `sh` are Unix only (module `with_mock_core`).

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use emu_runtime::DryRunRequest;
use emu_ui::{DRY_RUN_TIMEOUT, DryRunCommand, DryRunDir, DryRunVerdict, run_dry_run};

const EXE: &str = env!("CARGO_BIN_EXE_emu-rust");

struct Fixture {
    _root: tempfile::TempDir,
    cache: PathBuf,
    system: PathBuf,
    core: PathBuf,
    #[cfg_attr(not(unix), allow(dead_code))] // only the mock-core tests copy the game
    rom: PathBuf,
}

/// Directories and a game file; `core` is a placeholder (the mock core is built by `with_mock_core`).
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().canonicalize().unwrap();
    let cache = base.join("cache");
    let system = base.join("system");
    fs::create_dir_all(&system).unwrap();
    let rom = base.join("game.mock");
    fs::write(&rom, b"mock content").unwrap();
    Fixture {
        cache,
        core: system.join("placeholder-core"),
        system,
        rom,
        _root: root,
    }
}

fn command(env: &[(&str, &str)]) -> DryRunCommand {
    let mut command = DryRunCommand {
        program: PathBuf::from(EXE),
        args: vec![OsString::from("--dry-run-child")],
        env: Vec::new(),
    };
    command.env = env
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
    command
}

fn request(f: &Fixture, dry: &DryRunDir, content: Option<&Path>, frames: u32) -> DryRunRequest {
    DryRunRequest {
        core: f.core.clone(),
        content: content.map(Path::to_path_buf),
        system_dir: f.system.clone(),
        scratch_dir: dry.path().to_path_buf(),
        copy_dir: None,
        options: Vec::new(),
        frames,
    }
}

#[test]
fn a_bad_core_path_is_a_load_failure_with_a_message() {
    let f = fixture();
    let dry = DryRunDir::create(&f.cache).unwrap();
    let mut req = request(&f, &dry, None, 0);
    req.core = f.system.join("not-a-core.so");
    let verdict = run_dry_run(&command(&[]), &req, DRY_RUN_TIMEOUT);
    let DryRunVerdict::LoadFailed(message) = verdict else {
        panic!("{verdict:?}");
    };
    assert!(!message.is_empty());
}

#[test]
fn a_copy_dir_outside_the_dry_run_folder_is_refused() {
    let f = fixture();
    let dry = DryRunDir::create(&f.cache).unwrap();
    let mut req = request(&f, &dry, None, 0);
    req.copy_dir = Some(f.cache.join("core-copies"));
    assert_eq!(
        run_dry_run(&command(&[]), &req, DRY_RUN_TIMEOUT),
        DryRunVerdict::LoadFailed("copy_dir outside scratch_dir".to_owned())
    );
    assert!(!f.cache.join("core-copies").exists());
}

#[test]
fn the_child_alone_prints_nothing_and_a_bad_request_is_only_an_exit_code() {
    let f = fixture();
    let dry = DryRunDir::create(&f.cache).unwrap();
    let file = dry.path().join("request.toml");
    fs::write(&file, "this is not a request").unwrap();
    let out = Command::new(EXE)
        .arg("--dry-run-child")
        .arg(&file)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "no message on stdout");
    assert!(out.stderr.is_empty(), "no message on stderr");
}

#[cfg(unix)]
#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

#[cfg(unix)]
mod with_mock_core {
    use std::time::{Duration, Instant};

    use super::*;
    use emu_runtime::{DryRunOutcome, HwContextKind};
    use emu_ui::DRY_RUN_FRAMES;

    use super::mock_core::{self, Variant};

    fn mock_fixture() -> Fixture {
        Fixture {
            core: mock_core::build(Variant::Default),
            ..fixture()
        }
    }

    /// One dry run of the mock core on a copy of the game.
    fn run(env: &[(&str, &str)], frames: u32, timeout: Duration) -> DryRunVerdict {
        let f = mock_fixture();
        let dry = DryRunDir::create(&f.cache).unwrap();
        let game = dry.copy_game(&f.rom).unwrap();
        run_dry_run(
            &command(env),
            &request(&f, &dry, Some(&game), frames),
            timeout,
        )
    }

    #[test]
    fn a_healthy_core_passes_the_full_dry_run() {
        let DryRunVerdict::Passed(report) = run(&[], DRY_RUN_FRAMES, DRY_RUN_TIMEOUT) else {
            panic!("expected a pass");
        };
        assert_eq!(report.outcome, DryRunOutcome::Ran);
        assert_eq!(report.frames_run, DRY_RUN_FRAMES);
        assert!(!report.library_name.is_empty());
        assert!(report.extensions.iter().any(|e| e == "mock"));
    }

    #[test]
    fn a_probe_without_a_game_passes() {
        let f = mock_fixture();
        let dry = DryRunDir::create(&f.cache).unwrap();
        let verdict = run_dry_run(&command(&[]), &request(&f, &dry, None, 0), DRY_RUN_TIMEOUT);
        let DryRunVerdict::Passed(report) = verdict else {
            panic!("{verdict:?}");
        };
        assert_eq!(report.outcome, DryRunOutcome::Probed);
        assert_eq!(report.frames_run, 0);
    }

    #[test]
    fn descriptors_come_back_bounded_and_clean() {
        let DryRunVerdict::Passed(report) = run(&[("MOCK_DESCRIPTORS", "1")], 3, DRY_RUN_TIMEOUT)
        else {
            panic!("expected a pass");
        };
        assert!(!report.descriptors.is_empty());
        assert!(report.descriptors.len() <= emu_runtime::MAX_INPUT_DESCRIPTORS);
        assert!(
            report
                .descriptors
                .iter()
                .all(|d| d.description.chars().count() <= 64
                    && !d.description.chars().any(char::is_control))
        );
    }

    #[test]
    fn a_crashing_core_is_a_crash_verdict() {
        let verdict = run(&[("MOCK_CRASH_AT_FRAME", "3")], 50, DRY_RUN_TIMEOUT);
        assert!(
            matches!(verdict, DryRunVerdict::Crashed { .. }),
            "{verdict:?}"
        );
    }

    #[test]
    fn a_hanging_core_times_out_and_the_child_is_killed() {
        let started = Instant::now();
        let verdict = run(
            &[("MOCK_HANG_AT_FRAME", "0")],
            50,
            Duration::from_millis(1500),
        );
        assert_eq!(verdict, DryRunVerdict::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn a_required_hardware_render_is_reported_with_its_request() {
        let verdict = run(
            &[("MOCK_HW_RENDER", "3,3,2"), ("MOCK_HW_REQUIRED", "1")],
            10,
            DRY_RUN_TIMEOUT,
        );
        let DryRunVerdict::HardwareRender(hw) = verdict else {
            panic!("{verdict:?}");
        };
        assert_eq!(hw.context, HwContextKind::OpenGlCore);
        assert_eq!((hw.version_major, hw.version_minor), (3, 2));
    }

    #[test]
    fn a_core_that_floods_the_log_does_not_block_the_child() {
        let started = Instant::now();
        let verdict = run(&[("MOCK_LOG_FLOOD", "1")], 20, DRY_RUN_TIMEOUT);
        assert!(matches!(verdict, DryRunVerdict::Passed(_)), "{verdict:?}");
        assert!(started.elapsed() < Duration::from_secs(25));
    }

    #[test]
    fn the_game_is_never_modified_and_nothing_is_written_beside_the_folder() {
        let f = mock_fixture();
        let dry = DryRunDir::create(&f.cache).unwrap();
        let game = dry.copy_game(&f.rom).unwrap();
        let verdict = run_dry_run(
            &command(&[("MOCK_WRITE_CONTENT", "1")]),
            &request(&f, &dry, Some(&game), 5),
            DRY_RUN_TIMEOUT,
        );
        assert!(matches!(verdict, DryRunVerdict::Passed(_)), "{verdict:?}");
        assert_eq!(fs::read(&f.rom).unwrap(), b"mock content");
        assert!(
            fs::read(&game).unwrap().ends_with(b"mock-wrote\n"),
            "the copy was the one written"
        );
        let siblings: Vec<_> = fs::read_dir(f.cache.join("dry-run"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(siblings.len(), 1, "only the dry-run folder");
        let dry_path = dry.path().to_owned();
        drop(dry);
        assert!(!dry_path.exists(), "folder removed");
    }

    /// A child that ends with code 0 and writes nothing, or something unusable, is a crash.
    fn shell(script: &str) -> DryRunCommand {
        DryRunCommand {
            program: PathBuf::from("sh"),
            args: ["-c", script, "child"].map(OsString::from).to_vec(),
            env: Vec::new(),
        }
    }

    fn plain_request(f: &Fixture, dry: &DryRunDir) -> DryRunRequest {
        request(f, dry, None, 0)
    }

    #[test]
    fn code_zero_without_a_result_is_a_crash() {
        let f = mock_fixture();
        let dry = DryRunDir::create(&f.cache).unwrap();
        let verdict = run_dry_run(&shell("exit 0"), &plain_request(&f, &dry), DRY_RUN_TIMEOUT);
        assert_eq!(verdict, DryRunVerdict::Crashed { code: Some(0) });
    }

    #[test]
    fn a_nonzero_exit_is_a_crash_with_its_code() {
        let f = mock_fixture();
        let dry = DryRunDir::create(&f.cache).unwrap();
        let verdict = run_dry_run(&shell("exit 7"), &plain_request(&f, &dry), DRY_RUN_TIMEOUT);
        assert_eq!(verdict, DryRunVerdict::Crashed { code: Some(7) });
    }

    #[test]
    fn an_oversized_result_is_a_crash() {
        let f = mock_fixture();
        let dry = DryRunDir::create(&f.cache).unwrap();
        // `$1` is the request file: the result goes next to it.
        let script = r#"head -c 100000 /dev/zero | tr '\0' 'a' > "$(dirname "$1")/result.toml""#;
        let verdict = run_dry_run(&shell(script), &plain_request(&f, &dry), DRY_RUN_TIMEOUT);
        assert_eq!(verdict, DryRunVerdict::Crashed { code: Some(0) });
    }

    #[test]
    fn an_unreadable_result_is_a_crash() {
        let f = mock_fixture();
        let dry = DryRunDir::create(&f.cache).unwrap();
        let script = r#"echo 'outcome = "no-such-outcome"' > "$(dirname "$1")/result.toml""#;
        let verdict = run_dry_run(&shell(script), &plain_request(&f, &dry), DRY_RUN_TIMEOUT);
        assert_eq!(verdict, DryRunVerdict::Crashed { code: Some(0) });
    }

    #[test]
    fn a_stale_result_of_an_earlier_run_is_not_read() {
        let f = mock_fixture();
        let dry = DryRunDir::create(&f.cache).unwrap();
        fs::write(dry.path().join("result.toml"), "outcome = \"ran\"\n").unwrap();
        let verdict = run_dry_run(&shell("exit 0"), &plain_request(&f, &dry), DRY_RUN_TIMEOUT);
        assert_eq!(verdict, DryRunVerdict::Crashed { code: Some(0) });
    }

    #[test]
    fn the_child_ends_by_itself_when_the_core_hangs_and_nobody_waits() {
        let f = mock_fixture();
        let dry = DryRunDir::create(&f.cache).unwrap();
        let game = dry.copy_game(&f.rom).unwrap();
        let file = dry.path().join("request.toml");
        let q = |p: &Path| format!("'{}'", p.display());
        fs::write(
            &file,
            format!(
                "core = {}\ncontent = {}\nsystem_dir = {}\nscratch_dir = {}\nframes = 300\nwatchdog_secs = 1\n",
                q(&f.core),
                q(&game),
                q(&f.system),
                q(dry.path())
            ),
        )
        .unwrap();
        let started = Instant::now();
        // Nobody reads or waits for it: the child is a parent-less process as far as this test goes.
        let mut child = Command::new(EXE)
            .arg("--dry-run-child")
            .arg(&file)
            .env("MOCK_HANG_AT_FRAME", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > Duration::from_secs(20) {
                let _ = child.kill();
                panic!("the watchdog did not end the child");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(status.code(), Some(4), "watchdog exit code");
        assert!(started.elapsed() >= Duration::from_millis(900));
    }
}
