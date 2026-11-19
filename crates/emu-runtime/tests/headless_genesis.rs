//! Headless integration tests: genesis_plus_gx runs the 240p Test Suite (ADR 0007).
//!
//! All tests are `#[ignore]`; run `scripts/fetch-test-assets.sh` then
//! `cargo test -p emu-runtime -- --ignored`. Set `EMU_UPDATE_FINGERPRINTS=1` to (re)write
//! `tests/fingerprints/genesis_plus_gx-240p.txt` after inspecting the PNGs in `target/test-output/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    CollectSink, Emulator, EmulatorOptions, InputBackend, JoypadButton, NullInput, ScriptedInput,
};

/// One libretro core per process (ADR 0008): tests must not overlap.
static LOCK: Mutex<()> = Mutex::new(());

const CHECKPOINTS: [u64; 3] = [120, 300, 600];
const TOTAL_FRAMES: u64 = 600;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn core_path() -> PathBuf {
    root().join("tests/cores/genesis_plus_gx_libretro.so")
}

fn rom_path() -> PathBuf {
    root().join("tests/roms/240pSuite-1.21.bin")
}

fn fingerprint_path() -> PathBuf {
    root().join("tests/fingerprints/genesis_plus_gx-240p.txt")
}

fn output_dir() -> PathBuf {
    let dir = root().join("target/test-output");
    fs::create_dir_all(&dir).expect("create target/test-output");
    dir
}

fn lock() -> MutexGuard<'static, ()> {
    // A failed test poisons the mutex; the guarded state is `()`, so ignore it.
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn require(path: &Path) {
    assert!(
        path.is_file(),
        "missing test asset {}: lancer scripts/fetch-test-assets.sh",
        path.display()
    );
}

struct Session {
    /// (frame number, fingerprint) at each checkpoint.
    prints: Vec<(u64, u64)>,
    samples: usize,
    last_uniform: bool,
    sample_rate: f64,
    fps: f64,
}

fn emulator() -> Emulator {
    require(&core_path());
    require(&rom_path());
    let data = output_dir().join("data");
    let options = EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        core_variables: Vec::new(),
    };
    fs::create_dir_all(&options.system_dir).unwrap();
    fs::create_dir_all(&options.save_dir).unwrap();
    Emulator::load(&core_path(), &rom_path(), options).expect("load core and ROM")
}

/// Runs `TOTAL_FRAMES` frames; writes PNGs named `<tag>-<frame>.png` when `tag` is given.
fn run_session(input: &mut dyn InputBackend, tag: Option<&str>) -> Session {
    let mut emu = emulator();
    let mut audio = CollectSink::default();
    let av = emu.av_info();
    let (sample_rate, fps) = (av.timing.sample_rate, av.timing.fps);
    let mut prints = Vec::new();
    for _ in 0..TOTAL_FRAMES {
        emu.run_frame(input, &mut audio).expect("run_frame");
        let n = emu.frame_count();
        if CHECKPOINTS.contains(&n) {
            prints.push((n, emu.frame().fingerprint()));
            if let Some(tag) = tag {
                emu.frame()
                    .write_png(&output_dir().join(format!("{tag}-{n}.png")))
                    .expect("write png");
            }
        }
    }
    assert_eq!(emu.frame_count(), TOTAL_FRAMES);
    let px = emu.frame().pixels();
    let last_uniform = px.iter().enumerate().all(|(i, b)| *b == px[i % 4]);
    Session {
        prints,
        last_uniform,
        samples: audio.samples.len(),
        sample_rate,
        fps,
    }
}

fn scripted() -> ScriptedInput {
    ScriptedInput::new()
        .press(0, JoypadButton::Down, 400..410)
        .press(0, JoypadButton::A, 450..460)
        .press(0, JoypadButton::Start, 500..510)
}

fn core_hash() -> String {
    let path = root().join("tests/cores/genesis_plus_gx_libretro.so.sha256");
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn format_prints(prints: &[(u64, u64)]) -> String {
    let mut out = format!(
        "# genesis_plus_gx + 240pSuite-1.21.bin, no input; core sha256 {}\n",
        core_hash()
    );
    for (frame, fp) in prints {
        out.push_str(&format!("frame={frame} fingerprint={fp:016x}\n"));
    }
    out
}

fn parse_prints(text: &str) -> Vec<(u64, u64)> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut frame = None;
            let mut fp = None;
            for part in l.split_whitespace() {
                if let Some(v) = part.strip_prefix("frame=") {
                    frame = v.parse().ok();
                } else if let Some(v) = part.strip_prefix("fingerprint=") {
                    fp = u64::from_str_radix(v, 16).ok();
                }
            }
            (
                frame.unwrap_or_else(|| panic!("bad fingerprint line: {l}")),
                fp.unwrap_or_else(|| panic!("bad fingerprint line: {l}")),
            )
        })
        .collect()
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn av_info_matches_genesis() {
    let _guard = lock();
    let emu = emulator();
    assert!(
        emu.system_info().library_name.contains("Genesis Plus GX"),
        "unexpected core: {}",
        emu.system_info().library_name
    );
    // Genesis Plus GX reports 256x192 until the first frame sets the real mode.
    let av = emu.av_info();
    let (w, h) = (av.geometry.base_width, av.geometry.base_height);
    assert!(
        matches!((w, h), (320, 224) | (256, 224) | (256, 192)),
        "unexpected geometry {w}x{h}"
    );
    assert!((av.timing.fps - 59.9).abs() < 0.3, "fps {}", av.timing.fps);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn frames_match_fingerprints() {
    let _guard = lock();
    let session = run_session(&mut NullInput, Some("genesis-noinput"));

    assert!(!session.last_uniform, "frame at 600 is uniform");

    let path = fingerprint_path();
    if std::env::var_os("EMU_UPDATE_FINGERPRINTS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format_prints(&session.prints)).unwrap();
        eprintln!("fingerprints written to {}", path.display());
        return;
    }
    let text = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing {}: inspect target/test-output/*.png then run with EMU_UPDATE_FINGERPRINTS=1",
            path.display()
        )
    });
    let expected = parse_prints(&text);
    assert_eq!(
        session.prints,
        expected,
        "fingerprints differ (core sha256 now {}); inspect PNGs in target/test-output/, \
         regenerate with EMU_UPDATE_FINGERPRINTS=1 if the change is legitimate",
        core_hash()
    );
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn two_sessions_are_identical() {
    let _guard = lock();
    let a = run_session(&mut NullInput, None);
    let b = run_session(&mut NullInput, None);
    assert_eq!(a.prints, b.prints);
    assert_eq!(a.samples, b.samples);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn input_changes_the_picture() {
    let _guard = lock();
    let idle = run_session(&mut NullInput, None);
    let played = run_session(&mut scripted(), Some("genesis-input"));
    let last = |s: &Session| s.prints.last().copied().unwrap();
    assert_eq!(last(&idle).0, TOTAL_FRAMES);
    assert_ne!(
        last(&idle).1,
        last(&played).1,
        "scripted input had no visible effect at frame {TOTAL_FRAMES}"
    );
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn audio_sample_count_is_plausible() {
    let _guard = lock();
    let s = run_session(&mut NullInput, None);
    let expected = s.sample_rate / s.fps * TOTAL_FRAMES as f64 * 2.0;
    let got = s.samples as f64;
    assert!(
        (got - expected).abs() <= expected * 0.05,
        "got {got} samples, expected about {expected}"
    );
}
