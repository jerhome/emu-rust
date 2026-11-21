// The mock core only builds on Unix; on Windows most helpers are unused.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
//! Phase 2 end-to-end scenarios on the mock core (docs/design/phase2-frontend.md §3): `Pacer` + `RingSink` on a
//! simulated clock, pixel formats, core-driven option changes, savestates across processes, damaged `.srm`
//! files. Unix only (the mock core is built with the system C compiler). Not `#[ignore]`.
#![cfg(unix)]

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use emu_runtime::{
    AudioConfig, CollectSink, Emulator, EmulatorOptions, InputBackend, JoypadButton, MemoryKind,
    NullInput, PaceDecision, Pacer, RingReader, RingSink, ScriptedInput, Speed, SyncMode,
    audio_ring,
};
use mock_core::Variant;
use tempfile::TempDir;

/// One libretro core per process (ADR 0008): tests must not overlap.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const ROM: &[u8] = b"mock content: any bytes seed the image";

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
    let dir = root.join("roms");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Game (World).bin");
    fs::write(&path, ROM).unwrap();
    path
}

fn load(variant: Variant, root: &Path) -> Emulator {
    Emulator::load(&mock_core::build(variant), &rom(root), options(root)).expect("load mock core")
}

fn run(emu: &mut Emulator, input: &mut dyn InputBackend, n: usize) -> (Vec<u64>, Vec<i16>) {
    let mut audio = CollectSink::default();
    let prints = (0..n)
        .map(|_| {
            emu.run_frame(input, &mut audio).unwrap();
            emu.frame().fingerprint()
        })
        .collect();
    (prints, audio.samples)
}

// ---- Pacer + RingSink on a simulated clock ----

const DEVICE_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 512;
const FPS: f64 = 60.0;

/// Simulated frontend: 1 ms steps of a synthetic clock; the "device" reads fixed blocks as time passes.
struct Sim {
    emu: Emulator,
    sink: RingSink,
    reader: RingReader,
    pacer: Pacer,
    t0: Instant,
    elapsed: Duration,
    owed: f64,
    block: Vec<f32>,
    frames: u64,
}

impl Sim {
    fn new(emu: Emulator, mode: SyncMode) -> Self {
        let rate = emu.av_info().timing.sample_rate;
        let (sink, reader) = audio_ring(rate, DEVICE_RATE, &AudioConfig::default());
        let t0 = Instant::now();
        Self {
            emu,
            sink,
            reader,
            pacer: Pacer::new(FPS, mode, t0),
            t0,
            elapsed: Duration::ZERO,
            owed: 0.0,
            block: vec![0.0; BLOCK_FRAMES * 2],
            frames: 0,
        }
    }

    fn now(&self) -> Instant {
        self.t0 + self.elapsed
    }

    fn set_speed(&mut self, speed: Speed) {
        let now = self.now();
        self.pacer.set_speed(speed, now);
    }

    /// Advances `span` in 1 ms steps; returns the number of emulated frames run.
    fn advance(&mut self, span: Duration) -> u64 {
        let before = self.frames;
        let end = self.elapsed + span;
        while self.elapsed < end {
            self.elapsed += Duration::from_millis(1);
            self.owed += f64::from(DEVICE_RATE) / 1000.0;
            while self.owed >= BLOCK_FRAMES as f64 {
                self.owed -= BLOCK_FRAMES as f64;
                self.reader.fill(&mut self.block, 2);
            }
            // Same loop as the UI's `about_to_wait`: `Run` until the pacer says wait (bounded by MAX_BURST).
            loop {
                let now = self.now();
                match self.pacer.poll(now, self.sink.level()) {
                    PaceDecision::Run => {
                        self.emu.run_frame(&mut NullInput, &mut self.sink).unwrap();
                        self.frames += 1;
                    }
                    PaceDecision::WaitUntil(_) | PaceDecision::Idle => break,
                }
            }
        }
        self.frames - before
    }
}

#[test]
fn audio_sync_runs_at_core_rate_without_xruns() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let mut sim = Sim::new(load(Variant::Default, tmp.path()), SyncMode::Audio);
    let frames = sim.advance(Duration::from_secs(5));
    // 60 fps core; the pre-roll runs a few extra frames up front.
    assert!(
        (295..=312).contains(&frames),
        "{frames} frames in 5 simulated seconds"
    );
    let stats = sim.sink.stats();
    assert_eq!(stats.underruns, 0, "{stats:?}");
    assert_eq!(stats.dropped_samples, 0, "{stats:?}");
    let level = sim.sink.level().expect("audio level");
    assert!(
        level.buffered <= level.target + Duration::from_millis(40),
        "{level:?}"
    );
}

#[test]
fn clock_mode_runs_at_core_rate() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let mut sim = Sim::new(load(Variant::Default, tmp.path()), SyncMode::Clock);
    let frames = sim.advance(Duration::from_secs(5));
    assert!((298..=302).contains(&frames), "{frames}");
    assert_eq!(sim.sink.stats().dropped_samples, 0);
}

#[test]
fn pause_stops_frames_and_audio_without_xruns() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let mut sim = Sim::new(load(Variant::Default, tmp.path()), SyncMode::Audio);
    sim.advance(Duration::from_secs(2));

    sim.set_speed(Speed::Paused);
    sim.sink.set_paused(true);
    assert!(sim.sink.level().is_none(), "no level while paused");
    let during = sim.advance(Duration::from_secs(2));
    assert_eq!(during, 0, "no frame while paused");

    sim.sink.set_paused(false);
    sim.set_speed(Speed::Normal);
    let after = sim.advance(Duration::from_secs(3));
    assert!((175..=192).contains(&after), "{after} frames after resume");
    let stats = sim.sink.stats();
    assert_eq!(stats.underruns, 0, "{stats:?}");
    assert_eq!(stats.dropped_samples, 0, "{stats:?}");
}

#[test]
fn fast_forward_runs_four_times_faster_and_returns_to_normal() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let mut sim = Sim::new(load(Variant::Default, tmp.path()), SyncMode::Audio);
    sim.advance(Duration::from_secs(2));

    // The frontend mutes the sink during fast-forward (RingSink docs).
    sim.set_speed(Speed::FastForward { ratio: 4.0 });
    sim.sink.set_paused(true);
    let ff = sim.advance(Duration::from_secs(2));
    assert!((470..=490).contains(&ff), "{ff} frames in 2 s at 4x");

    sim.sink.set_paused(false);
    sim.set_speed(Speed::Normal);
    let normal = sim.advance(Duration::from_secs(3));
    assert!((175..=192).contains(&normal), "{normal} frames back at 1x");
    let stats = sim.sink.stats();
    assert_eq!(stats.underruns, 0, "{stats:?}");
    assert_eq!(stats.dropped_samples, 0, "{stats:?}");
}

// ---- Pixel formats end to end ----

fn expand5(v: u8) -> u8 {
    (v << 3) | (v >> 2)
}
fn expand6(v: u8) -> u8 {
    (v << 2) | (v >> 4)
}

#[test]
fn sixteen_bit_formats_match_the_quantized_reference() {
    let _guard = lock();
    for variant in [Variant::Rgb565, Variant::Xrgb1555] {
        let tmp = tempfile::tempdir().unwrap();
        // Reference: the same image as XRGB8888.
        let mut reference = Vec::new();
        {
            let mut emu = load(Variant::Default, tmp.path());
            for _ in 0..4 {
                run(&mut emu, &mut NullInput, 1);
                reference.push(emu.frame().pixels().to_vec());
            }
        }
        let mut emu = load(variant, tmp.path());
        for (i, want) in reference.iter().enumerate() {
            run(&mut emu, &mut NullInput, 1);
            let frame = emu.frame();
            assert_eq!((frame.width(), frame.height()), (64, 48), "{variant:?}");
            let got = frame.pixels();
            assert_eq!(got.len(), want.len());
            for (g, w) in got.as_chunks::<4>().0.iter().zip(want.as_chunks::<4>().0) {
                let expected = if variant == Variant::Rgb565 {
                    [expand5(w[0] >> 3), expand6(w[1] >> 2), expand5(w[2] >> 3)]
                } else {
                    [expand5(w[0] >> 3), expand5(w[1] >> 3), expand5(w[2] >> 3)]
                };
                assert_eq!(g[..3], expected, "{variant:?} frame {i}");
                assert_eq!(g[3], 255);
            }
        }
    }
}

// ---- Core-driven option change (SET_VARIABLE / GET_VARIABLE_UPDATE) ----

#[test]
fn core_set_variable_flags_options_changed_once() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let mut emu = load(Variant::DynamicOptions, tmp.path());
    let mut audio = CollectSink::default();
    let mut changed_at = Vec::new();
    for i in 0..30 {
        let status = emu.run_frame(&mut NullInput, &mut audio).unwrap();
        if status.options_changed {
            changed_at.push(i);
        }
    }
    assert_eq!(
        changed_at,
        [20],
        "options_changed on the frame of the change only"
    );
    let color = emu
        .options()
        .into_iter()
        .find(|o| o.key == "mock_color")
        .unwrap();
    assert_eq!(color.current, "blue");
    // A core-side change is not echoed back through GET_VARIABLE_UPDATE.
    assert!(
        !emu.take_messages()
            .iter()
            .any(|m| m.text.contains("variables updated"))
    );

    // Frontend-side change: the core sees GET_VARIABLE_UPDATE exactly once, and options_changed stays false.
    emu.set_option("mock_color", "green").unwrap();
    let mut flagged = false;
    for _ in 0..5 {
        flagged |= emu
            .run_frame(&mut NullInput, &mut audio)
            .unwrap()
            .options_changed;
    }
    assert!(!flagged);
    let updates = emu
        .take_messages()
        .iter()
        .filter(|m| m.text.contains("variables updated"))
        .count();
    assert_eq!(updates, 1);
}

#[test]
fn core_messages_carry_duration_and_repeat_after_reset() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    let mut emu = load(Variant::Default, tmp.path());
    run(&mut emu, &mut NullInput, 12);
    let first = emu.take_messages();
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(first[0].text, "mock: frame 10");
    // 60 frames at the core's 60 fps.
    assert!(
        first[0].duration >= Duration::from_millis(500)
            && first[0].duration <= Duration::from_secs(2),
        "{:?}",
        first[0].duration
    );
    emu.reset();
    run(&mut emu, &mut NullInput, 12);
    assert_eq!(emu.take_messages().len(), 1);
}

// ---- Savestate between two processes ----

const CHILD_DIR: &str = "EMU_TEST_CHILD_DIR";
const CHILD_ROLE: &str = "EMU_TEST_CHILD_ROLE";

fn fnv(samples: &[i16]) -> u64 {
    samples.iter().fold(0xcbf2_9ce4_8422_2325, |h, s| {
        (h ^ u64::from(*s as u16)).wrapping_mul(0x100_0000_01b3)
    })
}

/// Executed in a child process by `state_survives_a_new_process`; a no-op in a normal run.
#[test]
fn child_helper_for_cross_process_state() {
    let (Ok(dir), Ok(role)) = (std::env::var(CHILD_DIR), std::env::var(CHILD_ROLE)) else {
        return;
    };
    let root = PathBuf::from(dir);
    let mut emu = load(Variant::Default, &root);
    let out = match role.as_str() {
        "save" => {
            run(&mut emu, &mut NullInput, 20);
            emu.save_state(5).unwrap();
            root.join("expected.txt")
        }
        "load" => {
            emu.load_state(5).unwrap();
            root.join("actual.txt")
        }
        other => panic!("unknown role {other}"),
    };
    let (prints, audio) = run(&mut emu, &mut NullInput, 10);
    fs::write(out, format!("{prints:?} {:016x}", fnv(&audio))).unwrap();
}

fn child(dir: &Path, role: &str) {
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_helper_for_cross_process_state",
            "--test-threads=1",
        ])
        .env(CHILD_DIR, dir)
        .env(CHILD_ROLE, role)
        .status()
        .unwrap();
    assert!(status.success(), "child `{role}` failed");
}

#[test]
fn state_survives_a_new_process() {
    let _guard = lock();
    let tmp = tempfile::tempdir().unwrap();
    child(tmp.path(), "save");
    child(tmp.path(), "load");
    let expected = fs::read_to_string(tmp.path().join("expected.txt")).unwrap();
    let actual = fs::read_to_string(tmp.path().join("actual.txt")).unwrap();
    assert_eq!(expected, actual);
    assert!(expected.contains('['));
}

// ---- SRAM after a simulated crash ----

/// Writes a real `.srm` (counter 7 + marker), returns its path and content.
fn real_srm(root: &Path) -> (PathBuf, Vec<u8>) {
    let mut emu = load(Variant::Default, root);
    let mut start = ScriptedInput::new().press(0, JoypadButton::Start, 7..8);
    run(&mut emu, &mut start, 10);
    assert!(emu.flush_sram().unwrap());
    let path = emu.sram_path();
    let data = fs::read(&path).unwrap();
    assert_eq!(data.len(), 2048);
    (path, data)
}

#[test]
fn truncated_srm_restores_the_prefix_and_is_repaired() {
    let _guard = lock();
    let tmp = TempDir::new().unwrap();
    let (path, good) = real_srm(tmp.path());

    for cut in [0usize, 3, 5, 1000] {
        fs::write(&path, &good[..cut]).unwrap();
        let mut emu = load(Variant::Default, tmp.path());
        let sram = emu.memory(MemoryKind::SaveRam).unwrap();
        assert_eq!(&sram[..cut], &good[..cut], "cut {cut}");
        assert!(sram[cut..].iter().all(|b| *b == 0), "cut {cut}");
        run(&mut emu, &mut NullInput, 3);
        assert!(!emu.flush_sram().unwrap(), "nothing changed, cut {cut}");
        drop(emu);
        assert_eq!(fs::read(&path).unwrap().len(), cut, "left as found");
    }

    // The next real save rewrites a complete file.
    fs::write(&path, &good[..1000]).unwrap();
    let mut emu = load(Variant::Default, tmp.path());
    let mut start = ScriptedInput::new().press(0, JoypadButton::Start, 2..3);
    run(&mut emu, &mut start, 5);
    assert!(emu.flush_sram().unwrap());
    assert_eq!(fs::read(&path).unwrap().len(), 2048);
}

#[test]
fn oversized_srm_and_stale_temp_file_do_not_break_the_load() {
    let _guard = lock();
    let tmp = TempDir::new().unwrap();
    let (path, good) = real_srm(tmp.path());

    // Larger than the core's SRAM: the first 2048 bytes are used.
    let mut big = good.clone();
    big.extend(std::iter::repeat_n(0xEE, 4096));
    fs::write(&path, &big).unwrap();
    let emu = load(Variant::Default, tmp.path());
    assert_eq!(emu.memory(MemoryKind::SaveRam).unwrap(), good.as_slice());
    drop(emu);

    // Leftover temporary file of a crashed atomic write: ignored, the real file wins.
    let stale = path.parent().unwrap().join(format!(
        ".{}.tmp-1",
        path.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&stale, b"half written").unwrap();
    fs::write(&path, &good).unwrap();
    let emu = load(Variant::Default, tmp.path());
    assert_eq!(emu.memory(MemoryKind::SaveRam).unwrap(), good.as_slice());
}
