//! 8.0: hardware rendering is refused (three cases) and `dry_run`, against the mock core's v8.0 protocol
//! (`MOCK_HW_RENDER`, `MOCK_HW_REQUIRED`, `MOCK_HW_FRAME`, `MOCK_NO_FRAME`, `MOCK_DESCRIPTORS`, `MOCK_EXTENSIONS`).
#![allow(unsafe_code)]
#![cfg(unix)]

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use emu_runtime::{
    DryRunOutcome, DryRunRequest, Emulator, EmulatorOptions, HwContextKind, NullInput, NullSink,
    RuntimeError, dry_run, suppress_crash_dialogs,
};
use mock_core::Variant;

/// One core session at a time, and the environment is process-wide.
static SESSION: Mutex<()> = Mutex::new(());

/// Removes the variables when dropped, also when the test body panics.
struct EnvGuard(Vec<String>);

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for k in &self.0 {
            // SAFETY: still serialised by `SESSION` (the guard is dropped before the lock guard).
            unsafe { std::env::remove_var(k) };
        }
    }
}

/// Runs `body` with the given variables set, then removes them (even if `body` panics).
fn with_env<T>(vars: &[(&str, &str)], body: impl FnOnce(&Path) -> T) -> T {
    // A panicking test poisons the lock; the guard below already cleaned up, so the poison is harmless.
    let _lock = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let mut guard = EnvGuard(Vec::new());
    for (k, v) in vars {
        // SAFETY: serialised by `SESSION`; no other thread of this binary touches the environment.
        unsafe { std::env::set_var(k, v) };
        guard.0.push((*k).to_string());
    }
    let lib = mock_core::build(Variant::Default);
    body(&lib)
}

fn options(root: &Path) -> EmulatorOptions {
    let data = root.join("data");
    EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        cache_dir: data.join("cache"),
        provide_vfs: false,
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: Vec::new(),
        restore_added_disks: false,
        core_save_scope: emu_runtime::SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
        port_devices: Vec::new(),
    }
}

fn rom(root: &Path) -> PathBuf {
    let rom = root.join("game.bin");
    fs::write(&rom, b"mock content").unwrap();
    rom
}

fn load(lib: &Path, root: &Path) -> Result<Emulator, RuntimeError> {
    Emulator::load(lib, &rom(root), options(root))
}

fn run_n(emu: &mut Emulator, n: u32) -> Result<(), RuntimeError> {
    for _ in 0..n {
        emu.run_frame(&mut NullInput, &mut NullSink)?;
    }
    Ok(())
}

fn request(lib: &Path, root: &Path, content: Option<PathBuf>, frames: u32) -> DryRunRequest {
    DryRunRequest {
        core: lib.to_path_buf(),
        content,
        system_dir: root.join("system"),
        scratch_dir: root.join("scratch"),
        copy_dir: Some(root.join("scratch").join("core-copies")),
        options: Vec::new(),
        frames,
    }
}

fn is_hw(e: &RuntimeError) -> bool {
    matches!(e, RuntimeError::HardwareRenderRequired(_))
}

#[test]
fn software_core_is_unaffected() {
    with_env(&[], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let mut emu = load(lib, tmp.path()).unwrap();
        run_n(&mut emu, 100).unwrap();
    });
}

#[test]
fn load_refused_after_a_request_is_hardware_render_required() {
    with_env(
        &[("MOCK_HW_RENDER", "3,3,3"), ("MOCK_HW_REQUIRED", "1")],
        |lib| {
            let tmp = tempfile::tempdir().unwrap();
            let err = load(lib, tmp.path()).err().expect("load must fail");
            let RuntimeError::HardwareRenderRequired(hw) = err else {
                panic!("wrong error: {err}");
            };
            assert_eq!(hw.context, HwContextKind::OpenGlCore);
            assert_eq!((hw.version_major, hw.version_minor), (3, 3));
        },
    );
}

#[test]
fn request_alone_with_software_frames_is_allowed() {
    with_env(&[("MOCK_HW_RENDER", "3,3,3")], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let mut emu = load(lib, tmp.path()).unwrap();
        run_n(&mut emu, 120).unwrap();
    });
}

#[test]
fn sentinel_frame_is_hardware_render_required() {
    with_env(
        &[("MOCK_HW_RENDER", "6,1,1"), ("MOCK_HW_FRAME", "1")],
        |lib| {
            let tmp = tempfile::tempdir().unwrap();
            let mut emu = load(lib, tmp.path()).unwrap();
            let err = run_n(&mut emu, 5).unwrap_err();
            let RuntimeError::HardwareRenderRequired(hw) = err else {
                panic!("wrong error: {err}");
            };
            assert_eq!(hw.context, HwContextKind::Vulkan);
        },
    );
}

#[test]
fn sentinel_without_a_request_is_still_refused() {
    with_env(&[("MOCK_HW_FRAME", "1")], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let mut emu = load(lib, tmp.path()).unwrap();
        assert!(is_hw(&run_n(&mut emu, 1).unwrap_err()));
    });
}

#[test]
fn sixty_frames_without_a_software_buffer_after_a_request() {
    with_env(
        &[("MOCK_HW_RENDER", "3,3,3"), ("MOCK_NO_FRAME", "1")],
        |lib| {
            let tmp = tempfile::tempdir().unwrap();
            let mut emu = load(lib, tmp.path()).unwrap();
            run_n(&mut emu, 59).unwrap();
            assert!(is_hw(&run_n(&mut emu, 1).unwrap_err()));
        },
    );
}

#[test]
fn no_frame_without_a_request_is_not_judged() {
    with_env(&[("MOCK_NO_FRAME", "1")], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let mut emu = load(lib, tmp.path()).unwrap();
        run_n(&mut emu, 200).unwrap();
    });
}

#[test]
fn descriptors_reach_the_emulator() {
    with_env(&[("MOCK_DESCRIPTORS", "1")], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let emu = load(lib, tmp.path()).unwrap();
        let d = emu.input_descriptors();
        assert!(!d.is_empty() && d.len() <= emu_runtime::MAX_INPUT_DESCRIPTORS);
        assert_eq!(d[0].description, "Fire");
    });
}

#[test]
fn dry_run_probe_loads_the_core_only() {
    with_env(&[("MOCK_EXTENSIONS", "pce|sgx")], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let report = dry_run(&request(lib, tmp.path(), None, 300));
        assert_eq!(report.outcome, DryRunOutcome::Probed);
        assert_eq!(report.frames_run, 0);
        assert!(!report.library_name.is_empty());
        assert_eq!(report.extensions, ["pce", "sgx"]);
        assert!(!report.supports_no_game);
        assert!(report.hw_render.is_none());
    });
}

#[test]
fn dry_run_runs_300_frames_and_writes_only_under_scratch() {
    with_env(&[("MOCK_DESCRIPTORS", "1")], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let content = rom(tmp.path());
        fs::create_dir_all(tmp.path().join("system")).unwrap();
        let report = dry_run(&request(lib, tmp.path(), Some(content.clone()), 300));
        assert_eq!(report.outcome, DryRunOutcome::Ran, "{report:?}");
        assert_eq!(report.frames_run, 300);
        assert!(!report.descriptors.is_empty());
        assert!(report.hw_render.is_none());
        assert_eq!(fs::read(&content).unwrap(), b"mock content");
        let mut top: Vec<String> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        top.sort();
        assert_eq!(top, ["game.bin", "scratch", "system"]);
        assert_eq!(fs::read_dir(tmp.path().join("system")).unwrap().count(), 0);
    });
}

#[test]
fn dry_run_reports_a_refused_load_after_a_request() {
    with_env(
        &[("MOCK_HW_RENDER", "3,4,1"), ("MOCK_HW_REQUIRED", "1")],
        |lib| {
            let tmp = tempfile::tempdir().unwrap();
            let content = rom(tmp.path());
            let report = dry_run(&request(lib, tmp.path(), Some(content), 300));
            assert_eq!(report.outcome, DryRunOutcome::HardwareRenderRequired);
            let hw = report.hw_render.expect("request kept");
            assert_eq!((hw.version_major, hw.version_minor), (4, 1));
        },
    );
}

#[test]
fn dry_run_reports_a_sentinel_frame() {
    with_env(
        &[("MOCK_HW_RENDER", "3,3,3"), ("MOCK_HW_FRAME", "1")],
        |lib| {
            let tmp = tempfile::tempdir().unwrap();
            let content = rom(tmp.path());
            let report = dry_run(&request(lib, tmp.path(), Some(content), 300));
            assert_eq!(report.outcome, DryRunOutcome::HardwareRenderRequired);
            assert_eq!(report.frames_run, 0);
            assert!(report.hw_render.is_some());
        },
    );
}

#[test]
fn dry_run_reports_a_load_failure_without_panicking() {
    with_env(&[], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("absent.bin");
        let report = dry_run(&request(lib, tmp.path(), Some(missing), 10));
        assert!(
            matches!(report.outcome, DryRunOutcome::LoadFailed(_)),
            "{report:?}"
        );
        let report = dry_run(&request(
            &tmp.path().join("no-core.so"),
            tmp.path(),
            None,
            0,
        ));
        assert!(
            matches!(report.outcome, DryRunOutcome::LoadFailed(_)),
            "{report:?}"
        );
    });
}

#[test]
fn suppress_crash_dialogs_can_be_called_twice() {
    suppress_crash_dialogs();
    suppress_crash_dialogs();
}

/// Everything at the top of `root` except the fixtures, to prove nothing was written beside them.
fn top_level(root: &Path) -> Vec<String> {
    let mut top: Vec<String> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    top.sort();
    top
}

fn with_copy_dir(mut request: DryRunRequest, copy_dir: PathBuf) -> DryRunRequest {
    request.copy_dir = Some(copy_dir);
    request
}

fn assert_copy_dir_refused(report: &emu_runtime::DryRunReport) {
    assert_eq!(
        report.outcome,
        DryRunOutcome::LoadFailed("copy_dir outside scratch_dir".to_owned()),
        "{report:?}"
    );
}

#[test]
fn copy_dir_outside_scratch_is_refused_without_writing_anything() {
    with_env(&[], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let content = rom(tmp.path());
        fs::create_dir_all(tmp.path().join("system")).unwrap();
        for content in [None, Some(content)] {
            let base = request(lib, tmp.path(), content, 5);
            let outside = tmp.path().join("cache").join("core-copies");
            let report = dry_run(&with_copy_dir(base.clone(), outside));
            assert_copy_dir_refused(&report);
            // A sibling that only shares the prefix of the scratch folder is outside too.
            let report = dry_run(&with_copy_dir(base, tmp.path().join("scratch-other")));
            assert_copy_dir_refused(&report);
        }
        assert!(!tmp.path().join("cache").exists());
        assert!(!tmp.path().join("scratch-other").exists());
        assert_eq!(top_level(tmp.path()), ["game.bin", "system"]);
    });
}

#[test]
fn copy_dir_equal_to_scratch_or_escaping_it_is_refused() {
    with_env(&[], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let scratch = tmp.path().join("scratch");
        for dir in [scratch.clone(), scratch.join("..").join("elsewhere")] {
            let report = dry_run(&with_copy_dir(request(lib, tmp.path(), None, 0), dir));
            assert_copy_dir_refused(&report);
        }
        assert!(!tmp.path().join("elsewhere").exists());
    });
}

#[test]
fn copy_dir_inside_scratch_is_accepted_for_a_probe_and_for_a_run() {
    with_env(&[], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let content = rom(tmp.path());
        fs::create_dir_all(tmp.path().join("system")).unwrap();
        let private = tmp.path().join("scratch").join("private");
        let probe = with_copy_dir(request(lib, tmp.path(), None, 0), private.clone());
        assert_eq!(dry_run(&probe).outcome, DryRunOutcome::Probed);
        let run = with_copy_dir(request(lib, tmp.path(), Some(content), 5), private);
        assert_eq!(dry_run(&run).outcome, DryRunOutcome::Ran);
        assert_eq!(top_level(tmp.path()), ["game.bin", "scratch", "system"]);
    });
}

#[test]
fn the_private_core_copy_lands_in_the_accepted_copy_dir() {
    // A library that stays mapped after unloading (nodelete) is reloaded from a private copy: the first dry run
    // loads it directly, the second one must write its copy under the accepted copy_dir, and nowhere else.
    let _lock = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: serialised by `SESSION`; no other thread of this binary touches the environment.
    unsafe { std::env::set_var("MOCK_REFUSE_REINIT", "1") };
    let _env = EnvGuard(vec!["MOCK_REFUSE_REINIT".to_string()]);
    let lib = mock_core::build(Variant::NoDelete);
    let tmp = tempfile::tempdir().unwrap();
    let content = rom(tmp.path());
    let private = tmp.path().join("scratch").join("private");
    let mut reports = Vec::new();
    for _ in 0..2 {
        reports.push(dry_run(&with_copy_dir(
            request(&lib, tmp.path(), Some(content.clone()), 5),
            private.clone(),
        )));
    }
    for report in &reports {
        assert_eq!(report.outcome, DryRunOutcome::Ran, "{report:?}");
    }
    assert!(private.is_dir(), "no private copy folder was created");
    let mut top = top_level(tmp.path());
    top.retain(|n| n != "game.bin");
    assert_eq!(top, ["scratch"]);
}

#[test]
fn a_run_writes_nothing_outside_scratch_next_to_a_witness_folder() {
    with_env(&[], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let content = rom(tmp.path());
        fs::create_dir_all(tmp.path().join("system")).unwrap();
        fs::create_dir_all(tmp.path().join("witness")).unwrap();
        let report = dry_run(&with_copy_dir(
            request(lib, tmp.path(), Some(content), 5),
            tmp.path().join("scratch").join("copies"),
        ));
        assert_eq!(report.outcome, DryRunOutcome::Ran, "{report:?}");
        assert_eq!(fs::read_dir(tmp.path().join("witness")).unwrap().count(), 0);
        assert_eq!(
            top_level(tmp.path()),
            ["game.bin", "scratch", "system", "witness"]
        );
    });
}

#[test]
fn a_refused_request_does_not_create_the_scratch_folder() {
    with_env(&[], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let report = dry_run(&with_copy_dir(
            request(lib, tmp.path(), None, 0),
            tmp.path().join("outside"),
        ));
        assert_copy_dir_refused(&report);
        assert!(fs::read_dir(tmp.path()).unwrap().next().is_none());
    });
}

#[cfg(unix)]
#[test]
fn a_symlink_in_scratch_pointing_outside_is_refused() {
    with_env(&[], |lib| {
        let tmp = tempfile::tempdir().unwrap();
        let scratch = tmp.path().join("scratch");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&scratch).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, scratch.join("link")).unwrap();
        for dir in [scratch.join("link"), scratch.join("link").join("copies")] {
            let report = dry_run(&with_copy_dir(request(lib, tmp.path(), None, 0), dir));
            assert_copy_dir_refused(&report);
        }
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    });
}
