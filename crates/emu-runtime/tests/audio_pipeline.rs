//! Audio pipeline without a sound device (phase 1b, docs/design/phase1b-docker.md §2): `RingSink` on the
//! emulation side, `RingReader::fill` called at a simulated device cadence. The device clock is simulated:
//! after each emulated frame (1/fps s) the "device" consumes the matching number of frames, in fixed blocks.
//!
//! The core-driven tests are `#[ignore]` (`scripts/fetch-test-assets.sh`, then
//! `cargo test -p emu-runtime -- --ignored`); the synthetic-signal test runs everywhere.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use emu_runtime::{
    AudioConfig, AudioSink, CollectSink, Emulator, EmulatorOptions, NullInput, RingReader,
    RingSink, audio_ring,
};

/// One libretro core per process (ADR 0008): tests must not overlap.
static LOCK: Mutex<()> = Mutex::new(());

const DEVICE_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 512;
const CHANNELS: usize = 2;
const FRAMES: u64 = 600;

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn capacity_frames(config: &AudioConfig) -> f64 {
    2.0 * config.latency.as_secs_f64() * f64::from(DEVICE_RATE)
}

/// Simulated device: owns the reader and accumulates the time it must cover.
struct Device {
    reader: RingReader,
    /// Device frames owed (fractional) that have not been read yet.
    owed: f64,
    /// Device clock relative to the emulation clock (1.001 = device 0.1 % fast).
    skew: f64,
    block: Vec<f32>,
    /// Non-silent-pre-roll output (L, R interleaved), starting at the first non-zero block.
    played: Vec<f32>,
    started: bool,
    blocks_read: u64,
}

impl Device {
    fn new(reader: RingReader, skew: f64) -> Self {
        Self {
            reader,
            owed: 0.0,
            skew,
            block: vec![0.0; BLOCK_FRAMES * CHANNELS],
            played: Vec::new(),
            started: false,
            blocks_read: 0,
        }
    }

    /// Advances the device clock by `seconds` and reads every complete block that became due.
    fn advance(&mut self, seconds: f64) {
        self.owed += seconds * f64::from(DEVICE_RATE) * self.skew;
        while self.owed >= BLOCK_FRAMES as f64 {
            self.owed -= BLOCK_FRAMES as f64;
            self.reader.fill(&mut self.block, CHANNELS);
            self.blocks_read += 1;
            if !self.started && self.block.iter().any(|s| *s != 0.0) {
                self.started = true;
            }
            if self.started {
                self.played.extend_from_slice(&self.block);
            }
        }
    }
}

struct Run {
    /// (sink fill, underruns, dropped) after each frame.
    trace: Vec<(f64, u64, u64)>,
    peak_i16: i16,
    played: Vec<f32>,
    input_frames: usize,
    fill_end: f64,
}

fn emulator() -> Emulator {
    let core = root().join("tests/cores/genesis_plus_gx_libretro.so");
    let rom = root().join("tests/roms/240pSuite-1.21.bin");
    for p in [&core, &rom] {
        assert!(
            p.is_file(),
            "missing test asset {}: lancer scripts/fetch-test-assets.sh",
            p.display()
        );
    }
    let data = root().join("target/test-output/data");
    let options = EmulatorOptions {
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
    };
    std::fs::create_dir_all(&options.system_dir).unwrap();
    std::fs::create_dir_all(&options.save_dir).unwrap();
    Emulator::load(&core, &rom, options).expect("load core and ROM")
}

/// Tees the core's samples into the real sink and a side buffer (peak / count).
struct Tee<'a> {
    sink: &'a mut RingSink,
    seen: &'a mut CollectSink,
}

impl AudioSink for Tee<'_> {
    fn push_samples(&mut self, s: &[i16]) {
        self.seen.push_samples(s);
        self.sink.push_samples(s);
    }
}

fn run_core(skew: f64) -> Run {
    let mut emu = emulator();
    let av = emu.av_info();
    let (rate, fps) = (av.timing.sample_rate, av.timing.fps);
    let config = AudioConfig::default();
    let (mut sink, reader) = audio_ring(rate, DEVICE_RATE, &config);
    let mut device = Device::new(reader, skew);
    let mut seen = CollectSink::default();
    let mut trace = Vec::new();
    for _ in 0..FRAMES {
        let mut tee = Tee {
            sink: &mut sink,
            seen: &mut seen,
        };
        emu.run_frame(&mut NullInput, &mut tee).expect("run_frame");
        device.advance(1.0 / fps);
        let st = sink.stats();
        trace.push((sink.fill(), st.underruns, st.dropped_samples));
    }
    Run {
        trace,
        peak_i16: seen
            .samples
            .iter()
            .map(|s| s.saturating_abs())
            .max()
            .unwrap_or(0),
        played: device.played,
        input_frames: seen.samples.len() / 2,
        fill_end: sink.fill(),
    }
}

fn check_core_run(run: &Run, label: &str) {
    let last = run.trace.last().unwrap();
    eprintln!(
        "[{label}] input_frames={} peak={} fill_end={:.3} underruns={} dropped={} played_frames={}",
        run.input_frames,
        run.peak_i16,
        run.fill_end,
        last.1,
        last.2,
        run.played.len() / 2
    );
    // Steady state = after the first 200 frames (pre-roll + DRC settling).
    let steady = &run.trace[200..];
    assert_eq!(last.2, 0, "[{label}] ring overflow: samples dropped");
    assert!(
        last.1 - run.trace[199].1 <= 2,
        "[{label}] underruns in steady state: {} -> {}",
        run.trace[199].1,
        last.1
    );
    assert!(
        last.1 <= 4,
        "[{label}] total underruns {} not bounded",
        last.1
    );
    let (min, max) = steady
        .iter()
        .fold((1.0f64, 0.0f64), |(lo, hi), t| (lo.min(t.0), hi.max(t.0)));
    assert!(
        min > 0.2 && max < 0.8,
        "[{label}] fill left [0.2, 0.8] in steady state: {min:.3}..{max:.3}"
    );
    let tail = &run.trace[run.trace.len() - 100..];
    let mean = tail.iter().map(|t| t.0).sum::<f64>() / tail.len() as f64;
    assert!(
        (mean - 0.5).abs() < 0.15,
        "[{label}] fill does not converge to the set point: mean {mean:.3}"
    );
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn core_audio_through_ring_nominal_clock() {
    let _guard = lock();
    let run = run_core(1.0);
    check_core_run(&run, "nominal");
    // Rate sanity: 600 frames at 44.1 kHz / 59.92 fps = about 441 000 stereo frames.
    assert!(run.input_frames > 400_000, "core produced too little audio");
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn core_audio_through_ring_drifting_clock() {
    let _guard = lock();
    // Device clock 0.1 % fast, then 0.1 % slow: DRC (±0.5 %) must absorb both.
    for (skew, label) in [(1.001, "fast"), (0.999, "slow")] {
        let run = run_core(skew);
        check_core_run(&run, label);
    }
}

#[test]
#[ignore = "needs core and ROM: scripts/fetch-test-assets.sh"]
fn core_audio_signal_is_audible_when_core_produces_any() {
    let _guard = lock();
    let run = run_core(1.0);
    if run.peak_i16 == 0 {
        // Measured: the 240p Test Suite is silent after a start-up blip (peak 768 on frame 0).
        eprintln!("core output is silent over {FRAMES} frames; non-silence not required");
        return;
    }
    let peak = run.played.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        peak > 0.0,
        "core produced audio (peak {}) but the reader played only silence",
        run.peak_i16
    );
}

// ---- Synthetic signal, no core ------------------------------------------------------------------------------

const AMPLITUDE: f64 = 0.5;

/// L = sine, R = -sine, so a channel mix-up or a mono collapse would be visible.
fn sine_frames(n: usize, start: usize, rate: f64, freq: f64) -> Vec<i16> {
    let mut v = Vec::with_capacity(n * 2);
    for i in start..start + n {
        let s = (AMPLITUDE
            * (2.0 * std::f64::consts::PI * freq * i as f64 / rate + 0.3).sin()
            * f64::from(i16::MAX)) as i16;
        v.push(s);
        v.push(-s);
    }
    v
}

#[test]
fn synthetic_sine_is_continuous_and_resampled_at_the_right_ratio() {
    const INPUT_RATE: f64 = 32_040.0;
    const FREQ: f64 = 1_000.0;
    const CHUNK: usize = 534; // ~1/60 s of input
    const SECONDS: f64 = 4.0;

    let config = AudioConfig {
        latency: Duration::from_millis(64),
        ..AudioConfig::default()
    };
    let (mut sink, reader) = audio_ring(INPUT_RATE, DEVICE_RATE, &config);
    assert_eq!(sink.device_rate(), DEVICE_RATE);
    let mut device = Device::new(reader, 1.0);

    let chunks = (SECONDS * INPUT_RATE / CHUNK as f64) as usize;
    for c in 0..chunks {
        sink.push_samples(&sine_frames(CHUNK, c * CHUNK, INPUT_RATE, FREQ));
        device.advance(CHUNK as f64 / INPUT_RATE);
    }
    let input_frames = chunks * CHUNK;
    let stats = sink.stats();
    assert_eq!(stats.dropped_samples, 0, "overflow");
    assert_eq!(stats.underruns, 0, "underrun after pre-roll");
    assert!(!stats.stream_failed);

    // Pre-roll ended: a long signal was played.
    let frames_played = device.played.len() / 2;
    assert!(
        frames_played > (DEVICE_RATE as usize) * 3 / 2,
        "too little audio played"
    );

    // Continuity: no jump larger than a 1 kHz sine allows (max step 2π·f/rate·A ≈ 0.065), no mix-up of channels.
    let max_step = 0.1f32;
    let mut zero_run = 0usize;
    let pairs: Vec<[f32; 2]> = device.played.as_chunks::<2>().0.to_vec();
    for w in pairs.windows(2) {
        for ch in 0..2 {
            let step = (w[1][ch] - w[0][ch]).abs();
            assert!(step <= max_step, "discontinuity: step {step}");
        }
        assert!(
            (w[0][0] + w[0][1]).abs() < 1e-3,
            "L and R no longer mirror each other"
        );
        zero_run = if w[0][0] == 0.0 && w[1][0] == 0.0 {
            zero_run + 1
        } else {
            0
        };
        assert!(zero_run < 8, "silence gap inside the signal");
    }
    let peak = device.played.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((f64::from(peak) - AMPLITUDE).abs() < 0.02, "peak {peak}");

    // Frequency preserved: rising zero crossings of L, per second of played audio.
    let l: Vec<f32> = device.played.iter().step_by(2).copied().collect();
    let crossings = l.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    let measured = crossings as f64 / (l.len() as f64 / f64::from(DEVICE_RATE));
    assert!(
        (measured - FREQ).abs() / FREQ < 0.01 + 0.005,
        "frequency {measured} Hz instead of {FREQ}"
    );

    // Ratio end to end: frames written = frames played + frames still in the ring (+ the pre-roll fill that was
    // played as the first block). Overall within DRC bounds (±0.5 %) plus slack for block granularity.
    let in_ring = sink.fill() * capacity_frames(&config);
    let written = frames_played as f64 + in_ring;
    let expected = input_frames as f64 * f64::from(DEVICE_RATE) / INPUT_RATE;
    let rel = (written - expected).abs() / expected;
    eprintln!(
        "written={written:.0} expected={expected:.0} rel={rel:.5} fill={:.3}",
        sink.fill()
    );
    assert!(rel < 0.01, "resampling ratio off by {:.3} %", rel * 100.0);
}
