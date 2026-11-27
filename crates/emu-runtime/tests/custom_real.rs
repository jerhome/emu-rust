//! 8.0 custom systems, real cores (design 8.0 section 3 and 4, lot PQ2): `dry_run` against stella, mednafen_pce_fast
//! and flycast, the three cores the version is tried with. Software-rendered cores probe and run in the process; a
//! hardware-rendering core is refused (`HardwareRenderRequired`) without ever opening a window.
//!
//! Content: the Atari 2600 homebrew "Berta and Butterflies" (MIT) and the 240p Test Suite PC Engine / TurboGrafx-16
//! HuCard 1.10 (GPL-2.0), both pinned with their SHA-256 in `scripts/fetch-test-assets.sh`.
//!
//! No test feeds Stella a garbage file: on 2026-11-27 one run of such a test never came back (the core spins at 100 %
//! CPU), which only the child's watchdog of `emu-app` can stop.
//!
//! All tests are `#[ignore]`: run `scripts/fetch-test-assets.sh --with-8.0` then
//! `cargo test -p emu-runtime --test custom_real -- --ignored --test-threads=1`.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{DryRunOutcome, DryRunRequest, dry_run};

/// One libretro core per process at a time (ADR 0008).
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn require(path: PathBuf) -> PathBuf {
    assert!(
        path.is_file(),
        "missing test asset {}: lancer scripts/fetch-test-assets.sh --with-8.0",
        path.display()
    );
    path
}

fn core(name: &str) -> PathBuf {
    require(root().join(format!("tests/cores/{name}_libretro.so")))
}

fn berta() -> PathBuf {
    require(root().join("tests/roms/a2600/berta-and-butterflies.v1.00.ntsc.en.bin"))
}

fn pce_suite() -> PathBuf {
    require(root().join("tests/roms/pce/240pSuite-PCE-1.10.pce"))
}

/// A request whose every writable place is under `tmp` (`system/` is a fresh, empty folder).
fn request(core: PathBuf, tmp: &Path, content: Option<PathBuf>, frames: u32) -> DryRunRequest {
    let system = tmp.join("system");
    fs::create_dir_all(&system).unwrap();
    DryRunRequest {
        core,
        content,
        system_dir: system,
        scratch_dir: tmp.join("scratch"),
        copy_dir: Some(tmp.join("scratch").join("core-copies")),
        options: Vec::new(),
        frames,
    }
}

fn top_level(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
#[ignore = "needs scripts/fetch-test-assets.sh --with-8.0"]
fn stella_probe_reports_its_names_extensions_and_no_hardware_request() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let report = dry_run(&request(core("stella"), tmp.path(), None, 300));
    assert_eq!(report.outcome, DryRunOutcome::Probed, "{report:?}");
    assert_eq!(report.frames_run, 0);
    assert_eq!(report.library_name, "Stella");
    for ext in ["a26", "bin"] {
        assert!(
            report.extensions.iter().any(|e| e == ext),
            "{ext} in {:?}",
            report.extensions
        );
    }
    assert!(report.hw_render.is_none(), "software rendering");
    assert!(!report.supports_no_game);
}

#[test]
#[ignore = "needs scripts/fetch-test-assets.sh --with-8.0"]
fn stella_runs_300_frames_of_a_real_homebrew_twice_and_writes_only_under_scratch() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let content = berta();
    let before = fs::read(&content).unwrap();
    // Two loads one after the other, as the first launch does (essai à blanc, then the game; ADR 0022 section 7).
    for round in 1..=2 {
        let report = dry_run(&request(
            core("stella"),
            tmp.path(),
            Some(content.clone()),
            300,
        ));
        assert_eq!(
            report.outcome,
            DryRunOutcome::Ran,
            "round {round}: {report:?}"
        );
        assert_eq!(report.frames_run, 300, "round {round}");
        assert_eq!(report.library_name, "Stella");
        assert!(report.hw_render.is_none());
        // The 2600 controller descriptors are reported, bounded and clean.
        assert!(
            report.descriptors.len() <= 512,
            "{}",
            report.descriptors.len()
        );
    }
    assert_eq!(
        fs::read(&content).unwrap(),
        before,
        "the game is never written"
    );
    // Nothing next to the scratch folder except the system folder the caller made.
    assert_eq!(top_level(tmp.path()), ["scratch", "system"]);
}

#[test]
#[ignore = "needs scripts/fetch-test-assets.sh --with-8.0"]
fn mednafen_pce_fast_probe_gives_the_cartridge_extension_and_software_rendering() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let report = dry_run(&request(core("mednafen_pce_fast"), tmp.path(), None, 300));
    assert_eq!(report.outcome, DryRunOutcome::Probed, "{report:?}");
    assert!(
        report.extensions.iter().any(|e| e == "pce"),
        "{:?}",
        report.extensions
    );
    assert!(report.hw_render.is_none());
}

#[test]
#[ignore = "needs scripts/fetch-test-assets.sh --with-8.0"]
fn mednafen_pce_fast_runs_300_frames_of_a_real_homebrew_and_writes_only_under_scratch() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let content = pce_suite();
    let before = fs::read(&content).unwrap();
    let report = dry_run(&request(
        core("mednafen_pce_fast"),
        tmp.path(),
        Some(content.clone()),
        300,
    ));
    assert_eq!(report.outcome, DryRunOutcome::Ran, "{report:?}");
    assert_eq!(report.frames_run, 300);
    assert!(report.hw_render.is_none());
    assert!(report.descriptors.len() <= 512);
    assert_eq!(
        fs::read(&content).unwrap(),
        before,
        "the game is never written"
    );
    assert_eq!(top_level(tmp.path()), ["scratch", "system"]);
}

#[test]
#[ignore = "needs scripts/fetch-test-assets.sh --with-8.0"]
fn flycast_is_refused_for_hardware_rendering_and_opens_no_window() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    // Probe: the core loads (a hardware request may already be recorded, never honoured).
    let probe = dry_run(&request(core("flycast"), tmp.path(), None, 300));
    assert_eq!(probe.outcome, DryRunOutcome::Probed, "{probe:?}");
    // A content run (a dummy `.cdi`: Flycast asks for its GL context before reading the disc image) is refused
    // for rendering, or fails to load the dummy, but never runs a frame.
    let dummy = tmp.path().join("dummy.cdi");
    fs::write(&dummy, vec![0u8; 4096]).unwrap();
    let run = dry_run(&request(core("flycast"), tmp.path(), Some(dummy), 300));
    assert_eq!(run.frames_run, 0, "{run:?}");
    assert_eq!(
        run.outcome,
        DryRunOutcome::HardwareRenderRequired,
        "{run:?}"
    );
    // The request is kept (the core was dropped with the failed load): OpenGL 3.2 with the pinned Flycast.
    let hw = run.hw_render.expect("the request is reported");
    assert_eq!(format!("{:?}", hw.context), "OpenGl");
}
