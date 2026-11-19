//! Headless integration tests: genesis_plus_gx in Master System and Game Gear mode (ADR 0007).
//!
//! SMS: Sub Rescue 0.3 (MIT, animated gameplay). GG: Waternet 1.0 (MIT, native `.gg`, static menu).
//! All tests are `#[ignore]`; run
//! `scripts/fetch-test-assets.sh` then `cargo test -p emu-runtime --test headless_sms_gg -- --ignored`.
//! Set `EMU_UPDATE_FINGERPRINTS=1` to rewrite `tests/fingerprints/genesis_plus_gx-{sms,gg}.txt` after inspecting
//! the PNGs in `target/test-output/`.
//!
//! libretro -> console mapping measured with these ROMs: libretro B is console button 1 (Waternet menu: confirm)
//! and libretro A is button 2 (Waternet menu: no effect, "back"); Sub Rescue fires with either. On the Game Gear
//! libretro Start is the Start button (Waternet: confirm). Y, X, L, R and Select do nothing on either system;
//! libretro Start does nothing on the SMS (it is not wired to the pause button).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    CollectSink, Emulator, EmulatorOptions, InputBackend, InputFrame, JoypadButton, NullInput,
    ScriptedInput,
};

/// One libretro core per process (ADR 0008): tests must not overlap.
static LOCK: Mutex<()> = Mutex::new(());

const CHECKPOINTS: [u64; 3] = [120, 300, 600];
const TOTAL_FRAMES: u64 = 600;

#[derive(Clone, Copy)]
struct Target {
    tag: &'static str,
    rom: &'static str,
    width: u32,
    height: u32,
    /// Libretro aspect ratio reported by the core.
    aspect: f32,
    /// Frame at which the scripted button press starts.
    press_at: u64,
}

const SMS: Target = Target {
    tag: "sms",
    rom: "sub_rescue-0.3.sms",
    width: 256,
    height: 192,
    aspect: 1.5238,
    press_at: 200,
};

const GG: Target = Target {
    tag: "gg",
    rom: "waternet-1.0.gg",
    width: 160,
    height: 144,
    aspect: 4.0 / 3.0,
    press_at: 200,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn core_path() -> PathBuf {
    root().join("tests/cores/genesis_plus_gx_libretro.so")
}

fn rom_path(name: &str) -> PathBuf {
    root().join("tests/roms").join(name)
}

fn fingerprint_path(t: Target) -> PathBuf {
    root().join(format!("tests/fingerprints/genesis_plus_gx-{}.txt", t.tag))
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

fn load_in(data: &Path, rom: &Path) -> Emulator {
    require(&core_path());
    require(rom);
    let options = EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: Vec::new(),
    };
    fs::create_dir_all(&options.system_dir).unwrap();
    fs::create_dir_all(&options.save_dir).unwrap();
    Emulator::load(&core_path(), rom, options).expect("load core and content")
}

fn emulator(t: Target) -> Emulator {
    load_in(&output_dir().join("data"), &rom_path(t.rom))
}

struct Session {
    /// (frame number, fingerprint) at each checkpoint.
    prints: Vec<(u64, u64)>,
    samples: usize,
    last_uniform: bool,
    sample_rate: f64,
    fps: f64,
}

/// Runs `TOTAL_FRAMES` frames; writes PNGs named `<tag>-<frame>.png` when `tag` is given.
fn run_session(t: Target, input: &mut dyn InputBackend, tag: Option<&str>) -> Session {
    let mut emu = emulator(t);
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

fn core_hash() -> String {
    let path = root().join("tests/cores/genesis_plus_gx_libretro.so.sha256");
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn format_prints(t: Target, prints: &[(u64, u64)]) -> String {
    let mut out = format!(
        "# genesis_plus_gx + {}, no input; core sha256 {}\n",
        t.rom,
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

fn check_av_info(t: Target) {
    let _guard = lock();
    let emu = emulator(t);
    assert!(
        emu.system_info().library_name.contains("Genesis Plus GX"),
        "unexpected core: {}",
        emu.system_info().library_name
    );
    let av = emu.av_info();
    assert_eq!(
        (av.geometry.base_width, av.geometry.base_height),
        (t.width, t.height)
    );
    assert!(
        (av.geometry.aspect_ratio - t.aspect).abs() < 0.01,
        "aspect {}",
        av.geometry.aspect_ratio
    );
    assert!((av.timing.fps - 59.92).abs() < 0.3, "fps {}", av.timing.fps);
    assert!(
        (av.timing.sample_rate - 44100.0).abs() < 1.0,
        "sample rate {}",
        av.timing.sample_rate
    );
}

fn check_fingerprints(t: Target) {
    let _guard = lock();
    let session = run_session(t, &mut NullInput, Some(&format!("{}-noinput", t.tag)));
    assert!(!session.last_uniform, "frame at 600 is uniform");

    let path = fingerprint_path(t);
    if std::env::var_os("EMU_UPDATE_FINGERPRINTS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format_prints(t, &session.prints)).unwrap();
        eprintln!("fingerprints written to {}", path.display());
        return;
    }
    let text = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing {}: inspect target/test-output/*.png then run with EMU_UPDATE_FINGERPRINTS=1",
            path.display()
        )
    });
    assert_eq!(
        session.prints,
        parse_prints(&text),
        "fingerprints differ (core sha256 now {}); inspect PNGs in target/test-output/, \
         regenerate with EMU_UPDATE_FINGERPRINTS=1 if the change is legitimate",
        core_hash()
    );
}

fn check_frame_size(t: Target) {
    let _guard = lock();
    let mut emu = emulator(t);
    let mut audio = CollectSink::default();
    for _ in 0..60 {
        emu.run_frame(&mut NullInput, &mut audio)
            .expect("run_frame");
    }
    assert_eq!(
        (emu.frame().width(), emu.frame().height()),
        (t.width, t.height)
    );
}

fn press(t: Target, button: JoypadButton) -> ScriptedInput {
    ScriptedInput::new().press(0, button, t.press_at..t.press_at + 10)
}

/// Fingerprint 20 frames after the press started (a transient effect such as a torpedo is still on screen).
fn fingerprint_after(t: Target, input: &mut dyn InputBackend, png: Option<&str>) -> u64 {
    let mut emu = emulator(t);
    let mut audio = CollectSink::default();
    for _ in 0..t.press_at + 20 {
        emu.run_frame(input, &mut audio).expect("run_frame");
    }
    if let Some(tag) = png {
        emu.frame()
            .write_png(&output_dir().join(format!("{tag}.png")))
            .expect("write png");
    }
    emu.frame().fingerprint()
}

fn check_input(t: Target, effective: &[JoypadButton], ineffective: &[JoypadButton]) {
    let _guard = lock();
    let idle = fingerprint_after(t, &mut NullInput, None);
    for &b in effective {
        let played = fingerprint_after(t, &mut press(t, b), Some(&format!("{}-{b:?}", t.tag)));
        assert_ne!(idle, played, "{b:?} had no effect on {}", t.rom);
    }
    for &b in ineffective {
        let played = fingerprint_after(t, &mut press(t, b), None);
        assert_eq!(idle, played, "{b:?} unexpectedly changed {}", t.rom);
    }
}

fn check_audio(t: Target) {
    let _guard = lock();
    let s = run_session(t, &mut NullInput, None);
    let expected = s.sample_rate / s.fps * TOTAL_FRAMES as f64 * 2.0;
    let got = s.samples as f64;
    assert!(
        (got - expected).abs() <= expected * 0.05,
        "got {got} samples, expected about {expected}"
    );
}

/// Presents frames to `inner` as if counted from `start`, plus `base`, so a script can be replayed after a
/// state load.
struct Offset(ScriptedInput, u64, u64);

impl InputBackend for Offset {
    fn poll(&mut self, frame: u64, out: &mut InputFrame) {
        self.0.poll(frame - self.1 + self.2, out);
    }
}

/// Fingerprint after each frame, and the number of audio samples.
fn run_frames(emu: &mut Emulator, input: &mut dyn InputBackend, n: usize) -> (Vec<u64>, usize) {
    let mut audio = CollectSink::default();
    let prints = (0..n)
        .map(|_| {
            emu.run_frame(input, &mut audio).expect("run_frame");
            emu.frame().fingerprint()
        })
        .collect();
    (prints, audio.samples.len())
}

fn check_savestate(t: Target, state_suffix: &str) {
    let _guard = lock();
    let data = tempfile::tempdir().unwrap();
    let mut emu = load_in(data.path(), &rom_path(t.rom));
    assert_eq!(emu.core_id(), "genesis_plus_gx");
    run_frames(&mut emu, &mut NullInput, 300);
    let state = emu.serialize_state().expect("serialize");
    // A press after the snapshot makes the following frames differ from an idle run.
    let input = || ScriptedInput::new().press(0, JoypadButton::Down, 310..320);

    let start = emu.frame_count();
    let (prints, samples) = run_frames(&mut emu, &mut Offset(input(), start, 300), 60);
    emu.unserialize_state(&state).expect("unserialize");
    let start = emu.frame_count();
    let (again, samples_again) = run_frames(&mut emu, &mut Offset(input(), start, 300), 60);
    assert_eq!(prints, again, "image differs after unserialize");
    // The core does not serialize its resampler phase: compare the amount only, within a few stereo frames.
    assert!(
        samples.abs_diff(samples_again) <= 4,
        "audio sample count {samples} vs {samples_again}"
    );

    let path = emu.save_state(3).expect("save_state");
    assert!(
        path.ends_with(format!("states/genesis_plus_gx/{state_suffix}.state3")),
        "{}",
        path.display()
    );
    let start = emu.frame_count();
    let (prints, _) = run_frames(&mut emu, &mut Offset(input(), start, 300), 60);
    emu.load_state(3).expect("load_state");
    let start = emu.frame_count();
    let (again, _) = run_frames(&mut emu, &mut Offset(input(), start, 300), 60);
    assert_eq!(prints, again, "image differs after load_state");
    drop(emu);

    let mut emu = load_in(data.path(), &rom_path(t.rom));
    run_frames(&mut emu, &mut NullInput, 5);
    emu.load_state(3).expect("load_state in a new session");
    let start = emu.frame_count();
    let (again, _) = run_frames(&mut emu, &mut Offset(input(), start, 300), 60);
    assert_eq!(prints, again, "image differs in a second session");
}

// ---- Master System ----

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn sms_av_info() {
    check_av_info(SMS);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn sms_frame_size() {
    check_frame_size(SMS);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn sms_frames_match_fingerprints() {
    check_fingerprints(SMS);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn sms_two_sessions_are_identical() {
    let _guard = lock();
    let a = run_session(SMS, &mut NullInput, None);
    let b = run_session(SMS, &mut NullInput, None);
    assert_eq!(a.prints, b.prints);
    assert_eq!(a.samples, b.samples);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn sms_input_mapping() {
    // B and A are the two SMS fire buttons; the others do nothing in Sub Rescue. Directions move the sub.
    check_input(
        SMS,
        &[
            JoypadButton::B,
            JoypadButton::A,
            JoypadButton::Up,
            JoypadButton::Down,
            JoypadButton::Left,
            JoypadButton::Right,
        ],
        &[
            JoypadButton::Y,
            JoypadButton::X,
            JoypadButton::L,
            JoypadButton::R,
            JoypadButton::Select,
            JoypadButton::Start,
        ],
    );
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn sms_audio_sample_count_is_plausible() {
    check_audio(SMS);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn sms_savestate_round_trip() {
    check_savestate(SMS, "sub_rescue-0.3");
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn sms_zip_matches_rom() {
    let _guard = lock();
    let zip = rom_path("sub_rescue-0.3.zip");
    let data = tempfile::tempdir().unwrap();
    let reference = {
        let mut emu = load_in(data.path(), &rom_path(SMS.rom));
        run_frames(&mut emu, &mut NullInput, 300)
    };
    let mut emu = load_in(data.path(), &zip);
    assert_eq!(emu.game_name(), "sub_rescue-0.3");
    let got = run_frames(&mut emu, &mut NullInput, 300);
    assert_eq!(got.0, reference.0, "zip content renders differently");
    assert_eq!(got.1, reference.1);
}

// ---- Game Gear ----

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn gg_av_info() {
    check_av_info(GG);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn gg_frame_size() {
    check_frame_size(GG);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn gg_frames_match_fingerprints() {
    check_fingerprints(GG);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn gg_two_sessions_are_identical() {
    let _guard = lock();
    let a = run_session(GG, &mut NullInput, None);
    let b = run_session(GG, &mut NullInput, None);
    assert_eq!(a.prints, b.prints);
    assert_eq!(a.samples, b.samples);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn gg_input_mapping() {
    // Waternet main menu: Down moves the cursor, B (button 1) and Start confirm, A (button 2) does nothing.
    check_input(
        GG,
        &[JoypadButton::B, JoypadButton::Start, JoypadButton::Down],
        &[
            JoypadButton::A,
            JoypadButton::Y,
            JoypadButton::X,
            JoypadButton::L,
            JoypadButton::R,
            JoypadButton::Select,
        ],
    );
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn gg_audio_sample_count_is_plausible() {
    check_audio(GG);
}

/// Game Gear savestates with the nightly core: restoring brings back a blank (backdrop-only) picture instead of the
/// saved one, for good (a static menu never redraws). The SMS path and the Mega Drive are fine, so this is a
/// genesis_plus_gx defect, not a runtime one. This test pins the known bug (remove `should_panic` once fixed) ...
#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
#[should_panic(expected = "image differs after unserialize")]
fn gg_savestate_restores_picture_known_core_bug() {
    check_savestate(GG, "waternet-1.0");
}

/// ... and this one checks what still holds: the restored state is reproducible, from memory and from a file.
#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn gg_savestate_restore_is_reproducible() {
    let _guard = lock();
    let data = tempfile::tempdir().unwrap();
    let mut emu = load_in(data.path(), &rom_path(GG.rom));
    run_frames(&mut emu, &mut NullInput, 300);
    let state = emu.serialize_state().expect("serialize");
    emu.unserialize_state(&state).expect("unserialize");
    let first = run_frames(&mut emu, &mut NullInput, 30).0;
    emu.unserialize_state(&state).expect("unserialize again");
    assert_eq!(first, run_frames(&mut emu, &mut NullInput, 30).0);

    let path = emu.save_state(3).expect("save_state");
    assert!(path.ends_with("states/genesis_plus_gx/waternet-1.0.state3"));
    drop(emu);
    let mut emu = load_in(data.path(), &rom_path(GG.rom));
    run_frames(&mut emu, &mut NullInput, 5);
    emu.load_state(3).expect("load_state in a new session");
    assert_eq!(first, run_frames(&mut emu, &mut NullInput, 30).0);
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn gg_zip_matches_rom() {
    let _guard = lock();
    let zip = rom_path("Waternet-GameGear-1.0.zip");
    let data = tempfile::tempdir().unwrap();
    let reference = {
        let mut emu = load_in(data.path(), &rom_path(GG.rom));
        run_frames(&mut emu, &mut NullInput, 300)
    };
    let mut emu = load_in(data.path(), &zip);
    assert_eq!(emu.game_name(), "Waternet-GameGear-1.0");
    let got = run_frames(&mut emu, &mut NullInput, 300);
    assert_eq!(emu.frame().width(), 160);
    assert_eq!(got.0, reference.0, "zip content renders differently");
    assert_eq!(got.1, reference.1);
}
