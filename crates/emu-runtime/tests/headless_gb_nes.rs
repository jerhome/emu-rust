//! Headless integration tests: Game Boy / Game Boy Color (gambatte, sameboy, mgba) and NES (fceumm, nestopia,
//! mesen) cores (ADR 0007, `docs/design/v1.0-cores.md`).
//!
//! ROMs (all GPL-2.0 or MIT, pinned by `scripts/fetch-test-assets.sh`): `gb240p` and `240pee` from
//! 240p-test-mini v0.23 (menu, test screens, help pages; Artemio Urbina and Damian Yerrick), `cgb-acid2` 1.1
//! (Matt Currie, static colour rendering test). All tests are `#[ignore]`; run
//! `scripts/fetch-test-assets.sh` then `cargo test -p emu-runtime --test headless_gb_nes -- --ignored`.
//! Set `EMU_UPDATE_FINGERPRINTS=1` to rewrite `tests/fingerprints/<core>-<rom>.txt` after inspecting the PNGs in
//! `target/test-output/`.
//!
//! Cores that currently crash the process (SIGSEGV, not a panic) with this runtime are skipped unless
//! `EMU_TEST_BROKEN_CORES=1` is set; see `Target::broken`.
//!
//! Input mapping, measured on every core with the 240p test menus (libretro button -> console button):
//! - Game Boy: A -> A, B -> B, Start -> Start, Select -> Select, d-pad -> d-pad. L, R do nothing. Y and X are
//!   core-specific aliases (gambatte: Y = turbo B, X = turbo A; mgba: Y = B, X = A; sameboy: none) and are not
//!   asserted.
//! - NES: A -> A, B -> B, Start -> Start, Select -> Select, d-pad -> d-pad. L, R do nothing. Y and X are
//!   core-specific aliases and are not asserted.
//!
//! Screens used: the first page of both ROMs is a help page left with B. On the Game Boy the menu entry "Grid" is
//! reached with Down, A: there B exits, Start opens help, Select inverts the grays, A and the d-pad do nothing.
//! On the NES the first menu entry (PLUGE) is reached with A: B exits, Start opens help, Select switches the
//! pattern, A changes the background and Down darkens.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    CollectSink, Emulator, EmulatorOptions, InputBackend, InputFrame, JoypadButton, NullInput,
    ScriptedInput, StereoResampler,
};

/// One libretro core per process (ADR 0008): tests must not overlap.
static LOCK: Mutex<()> = Mutex::new(());

const CHECKPOINTS: [u64; 3] = [120, 300, 600];
const TOTAL_FRAMES: u64 = 600;

use JoypadButton as B;

/// Presses (button, first frame) replayed in every run of a scenario; each lasts 10 frames.
type Script = &'static [(JoypadButton, u64)];

/// Leaves the help page shown at start-up (SameBoy plays its boot animation for about 250 frames first).
const LEAVE_HELP: Script = &[(B::B, 300)];
/// Game Boy: help, then menu entry "Grid".
const GB_GRID: Script = &[(B::B, 300), (B::Down, 380), (B::A, 440)];
/// NES: help, then menu entry "PLUGE".
const NES_PLUGE: Script = &[(B::B, 300), (B::A, 380)];

#[derive(Clone, Copy)]
struct Scenario {
    setup: Script,
    /// Frame at which the tested button is pressed (10 frames); the picture is read 30 frames later.
    press_at: u64,
    effective: &'static [JoypadButton],
    ineffective: &'static [JoypadButton],
}

#[derive(Clone, Copy)]
struct Target {
    /// Core file stem without `_libretro.so`.
    core: &'static str,
    /// Short name of the ROM for fingerprint files.
    tag: &'static str,
    library: &'static str,
    rom: &'static str,
    width: u32,
    height: u32,
    /// Libretro aspect ratio reported by the core.
    aspect: f32,
    fps: f64,
    sample_rate: f64,
    scenario: Option<Scenario>,
    /// Why the core cannot run in this process yet, if so.
    broken: Option<&'static str>,
    /// Core option that changes the picture when set before the game loads: (key, value, expected size, applied
    /// at run time). Hardware-model options only take effect when the game loads.
    option: Option<(&'static str, &'static str, (u32, u32), bool)>,
}

const GB_SCENARIO: Scenario = Scenario {
    setup: GB_GRID,
    press_at: 560,
    effective: &[B::B, B::Select, B::Start],
    ineffective: &[B::A, B::Up, B::Down, B::Left, B::Right, B::L, B::R],
};

const NES_SCENARIO: Scenario = Scenario {
    setup: NES_PLUGE,
    press_at: 520,
    effective: &[B::B, B::A, B::Select, B::Start, B::Down],
    ineffective: &[B::Up, B::Left, B::Right, B::L, B::R],
};

const fn gb(core: &'static str, library: &'static str, sample_rate: f64) -> Target {
    Target {
        core,
        tag: "gb240p",
        library,
        rom: "gb240p-0.23.gb",
        width: 160,
        height: 144,
        aspect: 10.0 / 9.0,
        fps: 59.7275,
        sample_rate,
        scenario: Some(GB_SCENARIO),
        broken: None,
        option: None,
    }
}

const fn gbc(core: &'static str, library: &'static str, sample_rate: f64) -> Target {
    Target {
        tag: "cgbacid2",
        rom: "cgb-acid2-1.1.gbc",
        scenario: None,
        ..gb(core, library, sample_rate)
    }
}

const GAMBATTE: Target = Target {
    option: Some(("gambatte_gb_hwmode", "GB", (160, 144), false)),
    ..gb("gambatte", "Gambatte", 32768.0)
};
const SAMEBOY: Target = Target {
    option: Some(("sameboy_model", "Game Boy", (160, 144), false)),
    ..gb("sameboy", "SameBoy", 2_097_152.0)
};
const MGBA: Target = Target {
    option: Some(("mgba_gb_model", "Game Boy", (160, 144), false)),
    ..gb("mgba", "mGBA", 131_072.0)
};

const fn nes(
    core: &'static str,
    library: &'static str,
    size: (u32, u32),
    aspect: f32,
    fps: f64,
) -> Target {
    Target {
        core,
        tag: "240pee",
        library,
        rom: "240pee-0.23.nes",
        width: size.0,
        height: size.1,
        aspect,
        fps,
        sample_rate: 48000.0,
        scenario: Some(NES_SCENARIO),
        broken: None,
        option: None,
    }
}

const FCEUMM: Target = Target {
    option: Some(("fceumm_overscan_v_top", "0", (256, 232), true)),
    ..nes(
        "fceumm",
        "FCEUmm",
        (256, 224),
        8.0 / 7.0 * 256.0 / 224.0,
        60.0998,
    )
};
const NESTOPIA: Target = Target {
    // The nightly defaults to the Blargg NTSC composite filter (602 pixels wide).
    option: Some(("nestopia_blargg_ntsc_filter", "disabled", (256, 224), true)),
    broken: Some(
        "SIGSEGV in retro_run after the core has been loaded and unloaded a few times in one process",
    ),
    ..nes("nestopia", "Nestopia", (602, 224), 1.3163, 60.0988)
};
const MESEN: Target = Target {
    option: Some(("mesen_overscan_up", "8px", (256, 232), true)),
    ..nes(
        "mesen",
        "Mesen",
        (256, 240),
        8.0 / 7.0 * 256.0 / 240.0,
        60.0998,
    )
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn core_path(t: Target) -> PathBuf {
    root()
        .join("tests/cores")
        .join(format!("{}_libretro.so", t.core))
}

fn rom_path(name: &str) -> PathBuf {
    root().join("tests/roms").join(name)
}

fn fingerprint_path(t: Target) -> PathBuf {
    root().join(format!("tests/fingerprints/{}-{}.txt", t.core, t.tag))
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

/// `true` when the test must not run: the core is known to take the whole process down.
fn skip(t: Target) -> bool {
    match t.broken {
        Some(why) if std::env::var_os("EMU_TEST_BROKEN_CORES").is_none() => {
            eprintln!(
                "SKIPPED {}: {why} (EMU_TEST_BROKEN_CORES=1 to run anyway)",
                t.core
            );
            true
        }
        _ => false,
    }
}

fn require(path: &Path) {
    assert!(
        path.is_file(),
        "missing test asset {}: lancer scripts/fetch-test-assets.sh",
        path.display()
    );
}

fn load_in(t: Target, data: &Path, rom: &Path, variables: &[(&str, &str)]) -> Emulator {
    require(&core_path(t));
    require(rom);
    let options = EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: variables
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    };
    fs::create_dir_all(&options.system_dir).unwrap();
    fs::create_dir_all(&options.save_dir).unwrap();
    Emulator::load(&core_path(t), rom, options).expect("load core and content")
}

fn emulator(t: Target) -> Emulator {
    load_in(t, &output_dir().join("data"), &rom_path(t.rom), &[])
}

fn script(setup: Script) -> ScriptedInput {
    setup.iter().fold(ScriptedInput::new(), |input, &(b, at)| {
        input.press(0, b, at..at + 10)
    })
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

fn core_hash(t: Target) -> String {
    let path = root().join(format!("tests/cores/{}_libretro.so.sha256", t.core));
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn format_prints(t: Target, prints: &[(u64, u64)]) -> String {
    let mut out = format!(
        "# {} + {}, B at frame 300 on menu ROMs; core sha256 {}\n",
        t.core,
        t.rom,
        core_hash(t)
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
        emu.system_info().library_name.contains(t.library),
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
    assert!(
        (av.timing.fps - t.fps).abs() < 0.05,
        "fps {} expected {}",
        av.timing.fps,
        t.fps
    );
    assert!(
        (av.timing.sample_rate - t.sample_rate).abs() < 1.0,
        "sample rate {} expected {}",
        av.timing.sample_rate,
        t.sample_rate
    );
}

/// Menu ROMs start on a help page: B at frame 300 leaves it, so the checkpoints show help (120, 300) then the
/// menu (600). Static ROMs get no input.
fn fingerprint_input(t: Target) -> ScriptedInput {
    script(if t.scenario.is_some() {
        LEAVE_HELP
    } else {
        &[]
    })
}

fn check_fingerprints(t: Target) {
    let _guard = lock();
    let session = run_session(
        t,
        &mut fingerprint_input(t),
        Some(&format!("{}-{}", t.core, t.tag)),
    );
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
        core_hash(t)
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

fn check_two_sessions(t: Target) {
    let _guard = lock();
    let a = run_session(t, &mut fingerprint_input(t), None);
    let b = run_session(t, &mut fingerprint_input(t), None);
    assert_eq!(a.prints, b.prints);
    assert_eq!(a.samples, b.samples);
}

/// Fingerprint 30 frames after the tested press started (the effect is a stable screen by then).
fn fingerprint_after(
    t: Target,
    sc: Scenario,
    button: Option<JoypadButton>,
    png: Option<&str>,
) -> u64 {
    let mut emu = emulator(t);
    let mut audio = CollectSink::default();
    let mut input = script(sc.setup);
    if let Some(b) = button {
        input = input.press(0, b, sc.press_at..sc.press_at + 10);
    }
    for _ in 0..sc.press_at + 30 {
        emu.run_frame(&mut input, &mut audio).expect("run_frame");
    }
    if let Some(tag) = png {
        emu.frame()
            .write_png(&output_dir().join(format!("{tag}.png")))
            .expect("write png");
    }
    emu.frame().fingerprint()
}

fn check_input(t: Target) {
    let _guard = lock();
    let sc = t.scenario.expect("target without input scenario");
    let idle = fingerprint_after(t, sc, None, Some(&format!("{}-{}-idle", t.core, t.tag)));
    for &b in sc.effective {
        let png = format!("{}-{}-{b:?}", t.core, t.tag);
        let played = fingerprint_after(t, sc, Some(b), Some(&png));
        assert_ne!(
            idle, played,
            "{b:?} had no effect on {} / {}",
            t.core, t.rom
        );
    }
    for &b in sc.ineffective {
        let played = fingerprint_after(t, sc, Some(b), None);
        assert_eq!(
            idle, played,
            "{b:?} unexpectedly changed {} / {}",
            t.core, t.rom
        );
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

/// The runtime turns the core's rate into an output ratio: a rate it rejects would play at the wrong speed.
fn check_resampler_ratio(t: Target) {
    let _guard = lock();
    let rate = emulator(t).av_info().timing.sample_rate;
    let ratio = StereoResampler::new(rate, 48_000.0).ratio();
    assert!(
        (ratio - 48_000.0 / rate).abs() < 1e-9,
        "core rate {rate} Hz: resampler ratio {ratio}, expected {}",
        48_000.0 / rate
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

/// Frame at which the savestate is taken: the menu is up on every core.
const SNAPSHOT_AT: usize = 450;

fn check_savestate(t: Target, state_suffix: &str) {
    check_savestate_with(t, state_suffix, &[]);
}

fn check_savestate_with(t: Target, state_suffix: &str, variables: &[(&str, &str)]) {
    let _guard = lock();
    let data = tempfile::tempdir().unwrap();
    let rom = rom_path(t.rom);
    let mut emu = load_in(t, data.path(), &rom, variables);
    assert_eq!(emu.core_id(), t.core);
    run_frames(&mut emu, &mut script(LEAVE_HELP), SNAPSHOT_AT);
    let state = emu.serialize_state().expect("serialize");
    assert!(!state.is_empty());
    // A press after the snapshot (the menu cursor moves) makes the following frames differ from an idle run.
    let input = || ScriptedInput::new().press(0, B::Down, 460..470);
    let base = SNAPSHOT_AT as u64;

    let start = emu.frame_count();
    let (prints, samples) = run_frames(&mut emu, &mut Offset(input(), start, base), 60);
    emu.unserialize_state(&state).expect("unserialize");
    let start = emu.frame_count();
    let (again, samples_again) = run_frames(&mut emu, &mut Offset(input(), start, base), 60);
    assert_eq!(prints, again, "image differs after unserialize");
    // Resampler phases are not serialized by every core: compare the amount only, within a small margin.
    let margin = (samples / 1000).max(4);
    assert!(
        samples.abs_diff(samples_again) <= margin,
        "audio sample count {samples} vs {samples_again}"
    );

    let path = emu.save_state(3).expect("save_state");
    assert!(
        path.ends_with(format!("states/{}/{state_suffix}.state3", t.core)),
        "{}",
        path.display()
    );
    let start = emu.frame_count();
    let (prints, _) = run_frames(&mut emu, &mut Offset(input(), start, base), 60);
    emu.load_state(3).expect("load_state");
    let start = emu.frame_count();
    let (again, _) = run_frames(&mut emu, &mut Offset(input(), start, base), 60);
    assert_eq!(prints, again, "image differs after load_state");
    drop(emu);

    let mut emu = load_in(t, data.path(), &rom, variables);
    run_frames(&mut emu, &mut NullInput, 5);
    emu.load_state(3).expect("load_state in a new session");
    let start = emu.frame_count();
    let (again, _) = run_frames(&mut emu, &mut Offset(input(), start, base), 60);
    assert_eq!(prints, again, "image differs in a second session");
}

/// Stored-only zip holding `member` (name and content), written with the `zip` crate.
fn write_zip(path: &Path, member: &str, content: &[u8]) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file(
        member,
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(content).unwrap();
    zip.finish().unwrap();
}

fn check_zip(t: Target) {
    let _guard = lock();
    let data = tempfile::tempdir().unwrap();
    let rom = rom_path(t.rom);
    let stem = t.rom.rsplit_once('.').unwrap().0;
    let zip = data.path().join(format!("{stem}.zip"));
    write_zip(&zip, t.rom, &fs::read(&rom).unwrap());
    let reference = {
        let mut emu = load_in(t, data.path(), &rom, &[]);
        run_frames(&mut emu, &mut NullInput, 300)
    };
    let mut emu = load_in(t, data.path(), &zip, &[]);
    assert_eq!(emu.game_name(), stem);
    let got = run_frames(&mut emu, &mut NullInput, 300);
    assert_eq!(emu.frame().width(), t.width);
    assert_eq!(got.0, reference.0, "zip content renders differently");
    assert_eq!(got.1, reference.1);
}

/// Options: listed with their default, applied at load, changeable at run time.
fn check_options(t: Target) {
    let _guard = lock();
    let (key, value, size, live) = t.option.expect("target without option check");
    let data = tempfile::tempdir().unwrap();
    let rom = rom_path(t.rom);

    let mut emu = load_in(t, data.path(), &rom, &[]);
    let options = emu.options();
    let opt = options
        .iter()
        .find(|o| o.key == key)
        .unwrap_or_else(|| panic!("option {key} not declared by {}", t.core));
    assert_eq!(opt.current, opt.default, "{key} starts at its default");
    assert!(
        opt.values.iter().any(|v| v.value == value),
        "{key}: {value}?"
    );
    assert!(
        emu.set_option(key, "no such value").is_err(),
        "invalid value accepted"
    );
    assert!(
        emu.set_option("no_such_option", value).is_err(),
        "unknown key accepted"
    );
    run_frames(&mut emu, &mut script(LEAVE_HELP), 460);
    let before = emu.frame().fingerprint();
    drop(emu);

    // At load (core_variables).
    let mut emu = load_in(t, data.path(), &rom, &[(key, value)]);
    assert_eq!(
        emu.options().iter().find(|o| o.key == key).unwrap().current,
        value
    );
    run_frames(&mut emu, &mut script(LEAVE_HELP), 460);
    let after = emu.frame().fingerprint();
    assert_ne!(before, after, "{key}={value} did not change the picture");
    assert_eq!((emu.frame().width(), emu.frame().height()), size);
    emu.frame()
        .write_png(&output_dir().join(format!("{}-{}-option.png", t.core, t.tag)))
        .unwrap();
    drop(emu);

    // At run time (set_option).
    let mut emu = load_in(t, data.path(), &rom, &[]);
    run_frames(&mut emu, &mut script(LEAVE_HELP), 400);
    emu.set_option(key, value).expect("set_option");
    assert_eq!(
        emu.options().iter().find(|o| o.key == key).unwrap().current,
        value
    );
    run_frames(&mut emu, &mut NullInput, 60);
    if !live {
        return;
    }
    assert_ne!(
        before,
        emu.frame().fingerprint(),
        "{key}={value} set at run time did not change the picture"
    );
    assert_eq!((emu.frame().width(), emu.frame().height()), size);
}

macro_rules! suite {
    ($name:ident, $target:expr, $($extra:item)*) => {
        mod $name {
            use super::*;
            const T: Target = $target;

            #[test]
            #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
            fn av_info() {
                if !skip(T) { check_av_info(T) }
            }

            #[test]
            #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
            fn frame_size() {
                if !skip(T) { check_frame_size(T) }
            }

            #[test]
            #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
            fn frames_match_fingerprints() {
                if !skip(T) { check_fingerprints(T) }
            }

            #[test]
            #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
            fn two_sessions_are_identical() {
                if !skip(T) { check_two_sessions(T) }
            }

            #[test]
            #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
            fn audio_sample_count_is_plausible() {
                if !skip(T) { check_audio(T) }
            }

            #[test]
            #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
            fn zip_matches_rom() {
                if !skip(T) { check_zip(T) }
            }

            $($extra)*
        }
    };
}

/// Tests that need the menu ROMs: input, options.
macro_rules! interactive {
    () => {
        #[test]
        #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
        fn input_mapping() {
            if !skip(T) {
                check_input(T)
            }
        }

        #[test]
        #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
        fn option_changes_picture() {
            if !skip(T) {
                check_options(T)
            }
        }
    };
}

macro_rules! savestate {
    ($suffix:expr) => {
        #[test]
        #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
        fn savestate_round_trip() {
            if !skip(T) {
                check_savestate(T, $suffix)
            }
        }
    };
}

/// The runtime derives the resampling ratio from the core's rate.
macro_rules! resampler {
    () => {
        #[test]
        #[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
        fn resampler_ratio() {
            if !skip(T) {
                check_resampler_ratio(T)
            }
        }
    };
}

// ---- Game Boy, menu ROM ----
suite!(gambatte_gb, GAMBATTE, interactive!(); savestate!("gb240p-0.23"); resampler!(););
suite!(sameboy_gb, SAMEBOY, interactive!(); savestate!("gb240p-0.23"););
suite!(mgba_gb, MGBA, interactive!(); savestate!("gb240p-0.23"); resampler!(););

// ---- Game Boy Color, static colour test ----
suite!(gambatte_gbc, gbc("gambatte", "Gambatte", 32768.0),);
suite!(sameboy_gbc, gbc("sameboy", "SameBoy", 2_097_152.0),);
suite!(mgba_gbc, gbc("mgba", "mGBA", 131_072.0),);

// ---- NES ----
suite!(fceumm_nes, FCEUMM, interactive!(); savestate!("240pee-0.23"); resampler!(););
suite!(nestopia_nes, NESTOPIA, interactive!(); resampler!(););
suite!(mesen_nes, MESEN, interactive!(); savestate!("240pee-0.23"); resampler!(););

/// SameBoy reports a 2,097,152 Hz sample rate, above `emu_runtime::resample::MAX_RATE` (1 MHz): the runtime
/// falls back to 44.1 kHz and would play the core's audio at the wrong speed. Known runtime defect: remove
/// `should_panic` once rates above 1 MHz are supported.
#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
#[should_panic(expected = "resampler ratio")]
fn sameboy_resampler_ratio_known_runtime_bug() {
    check_resampler_ratio(SAMEBOY);
}

/// Nestopia with its default Blargg NTSC filter: the filter phase alternates every frame and is not part of the
/// savestate, so the replay after a restore is one frame out of phase. Known core behaviour.
#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
#[should_panic(expected = "image differs after unserialize")]
fn nestopia_savestate_with_ntsc_filter_known_core_quirk() {
    if skip(NESTOPIA) {
        panic!("image differs after unserialize (skipped)");
    }
    check_savestate(NESTOPIA, "240pee-0.23");
}

/// Same round trip with the filter off: exact.
#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn nestopia_savestate_round_trip_without_filter() {
    if !skip(NESTOPIA) {
        check_savestate_with(
            NESTOPIA,
            "240pee-0.23",
            &[("nestopia_blargg_ntsc_filter", "disabled")],
        );
    }
}
