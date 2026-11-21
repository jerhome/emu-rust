//! Generic reload test (ADR 0022, rule 7): for every real core present in `tests/cores/`, two successive loads in
//! the same process, each running N frames; the last frame of the second session must equal the first one's.
//! Each core runs in a child process (this binary re-executed, `zz_worker`), so a crash is a named failure
//! instead of the death of the test binary.
//!
//! `#[ignore]`: run `scripts/fetch-test-assets.sh --with-all` then
//! `cargo test -p emu-libretro --test reload_real -- --ignored --test-threads=1`.
//! A core whose library file is absent is reported and skipped; cap32 and vice_x64 may also be skipped when
//! they refuse to start without their own system files (flagged `optional`).
//! dosbox_pure must go through the private copy (the `info!` journal line is captured by the worker).

#![cfg(target_os = "linux")]
// Linking libm needs an `extern` block (test binary only; emu-libretro is allowed `unsafe`).
#![allow(unsafe_code)]

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use emu_libretro::{Core, CoreConfig, FrontendHandler, InputQuery, PixelFormat, VideoFrame};

const FRAMES: u32 = 300;

// puae cores do not list libm as a dependency and expect the host to provide it; this test binary must link it
// (a referenced symbol keeps `--as-needed` from dropping it).
#[cfg(target_os = "linux")]
#[link(name = "m")]
unsafe extern "C" {
    safe fn cosf(x: f32) -> f32;
}

struct Case {
    core: &'static str,
    rom: &'static str,
    /// Installs `tests/roms/system/tos.img` as `system/tos.img` (Hatari).
    tos: bool,
    /// Missing system files may make the core refuse to start: skip instead of failing.
    optional: bool,
    /// The library must stay resident and be reloaded through a private copy.
    resident: bool,
}

const fn case(core: &'static str, rom: &'static str) -> Case {
    Case {
        core,
        rom,
        tos: false,
        optional: false,
        resident: false,
    }
}

fn cases() -> Vec<Case> {
    vec![
        case("genesis_plus_gx", "240pSuite-1.21.bin"),
        case("snes9x", "240pSuite-SNES-1.02.sfc"),
        case("gambatte", "gb240p-0.23.gb"),
        case("mesen", "240pee-0.23.nes"),
        case("fceumm", "240pee-0.23.nes"),
        case("nestopia", "240pee-0.23.nes"),
        case("mgba", "gba/hello.gba"),
        case("sameboy", "cgb-acid2-1.1.gbc"),
        case("picodrive", "240pSuite-1.21.bin"),
        Case {
            tos: true,
            ..case("hatari", "nova-defense.st")
        },
        case("puae", "amiga/boot.adf"),
        case("puae2021", "amiga/boot.adf"),
        Case {
            resident: true,
            ..case("dosbox_pure", "dos/matrix62.zip")
        },
        Case {
            optional: true,
            ..case("cap32", "cpc/rpg.dsk")
        },
        Case {
            optional: true,
            ..case("vice_x64", "c64/uberswap.prg")
        },
    ]
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ---- worker side ----

/// Keeps a hash of the last frame (visible pixels only).
#[derive(Default)]
struct Probe {
    last: Option<u64>,
}

impl FrontendHandler for Probe {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
        let Some(f) = frame else { return };
        let bpp = if f.format == PixelFormat::Xrgb8888 {
            4
        } else {
            2
        };
        let mut h = std::hash::DefaultHasher::new();
        (f.width, f.height, bpp).hash(&mut h);
        for row in 0..f.height as usize {
            let start = row * f.pitch;
            f.data[start..start + f.width as usize * bpp].hash(&mut h);
        }
        self.last = Some(h.finish());
    }
    fn audio_samples(&mut self, _samples: &[i16]) {}
    fn input_poll(&mut self) {}
    fn input_state(&mut self, _query: InputQuery) -> i16 {
        0
    }
}

/// Captures the `info!` events of `emu-libretro` (the "stayed in memory" line).
#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<String>>>);

struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for Journal {
    fn enabled(&self, m: &tracing::Metadata<'_>) -> bool {
        *m.level() <= tracing::Level::INFO
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        let mut m = Message(String::new());
        e.record(&mut m);
        if e.metadata().level() == &tracing::Level::INFO {
            self.0.lock().unwrap().push(m.0);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn session(core: &Path, rom: &Path, dir: &Path, label: &str) -> u64 {
    let config = CoreConfig {
        system_dir: dir.join("system"),
        save_dir: dir.join("saves"),
        assets_dir: dir.join("assets"),
        variables: Vec::new(),
        provide_vfs: false,
        copy_dir: Some(dir.join("copies")),
    };
    let mut c = Core::load(core, config).unwrap_or_else(|e| panic!("{label}: load: {e}"));
    c.load_game(rom)
        .unwrap_or_else(|e| panic!("{label}: load_game: {e}"));
    let mut probe = Probe::default();
    for _ in 0..FRAMES {
        c.run(&mut probe)
            .unwrap_or_else(|e| panic!("{label}: run: {e}"));
    }
    probe
        .last
        .unwrap_or_else(|| panic!("{label}: no frame produced"))
    // `c` dropped here: retro_deinit + library close.
}

#[test]
#[ignore = "worker of `reload_every_real_core`; needs RELOAD_CASE"]
fn zz_worker() {
    let Some(name) = std::env::var_os("RELOAD_CASE") else {
        return;
    };
    #[cfg(target_os = "linux")]
    std::hint::black_box(cosf(std::hint::black_box(0.5)));
    let name = name.to_string_lossy().into_owned();
    let case = cases().into_iter().find(|c| c.core == name).unwrap();
    let journal = Journal::default();
    tracing::subscriber::set_global_default(journal.clone()).unwrap();

    let dir = root().join("target/test-output/reload").join(case.core);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("system")).unwrap();
    std::fs::create_dir_all(dir.join("saves")).unwrap();
    if case.tos {
        std::fs::copy(
            root().join("tests/roms/system/tos.img"),
            dir.join("system/tos.img"),
        )
        .unwrap();
    }
    let lib = root().join(format!("tests/cores/{}_libretro.so", case.core));
    let rom = root().join("tests/roms").join(case.rom);
    let first = session(&lib, &rom, &dir, "first session");
    let second = session(&lib, &rom, &dir, "second session");
    let resident = journal
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|m| m.contains("stayed in memory"));
    let leftovers = std::fs::read_dir(dir.join("copies")).map_or(0, Iterator::count);
    println!();
    println!("@@ first={first:016x}");
    println!("@@ second={second:016x}");
    println!("@@ resident={}", u8::from(resident));
    println!("@@ leftovers={leftovers}");
}

// ---- driver side ----

#[test]
#[ignore = "needs tests/cores (scripts/fetch-test-assets.sh --with-all)"]
#[cfg(target_os = "linux")]
fn reload_every_real_core() {
    let mut failures = Vec::new();
    let mut ran = 0;
    for case in cases() {
        let lib = root().join(format!("tests/cores/{}_libretro.so", case.core));
        let rom = root().join("tests/roms").join(case.rom);
        if !lib.is_file() || !rom.is_file() {
            eprintln!("SKIP {}: {} or its ROM missing", case.core, lib.display());
            assert!(
                case.core != "dosbox_pure",
                "dosbox_pure and its ROM are mandatory: run fetch-test-assets.sh --with-4.0"
            );
            continue;
        }
        let run = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "zz_worker",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("RELOAD_CASE", case.core)
            .output()
            .expect("spawn the worker");
        let stdout = String::from_utf8_lossy(&run.stdout);
        let stderr = String::from_utf8_lossy(&run.stderr);
        if !run.status.success() {
            if case.optional && stderr.contains("first session: load") {
                eprintln!("SKIP {}: refuses to start here\n{stderr}", case.core);
                continue;
            }
            failures.push(format!("{}: worker {}\n{stderr}", case.core, run.status));
            continue;
        }
        let report: BTreeMap<&str, &str> = stdout
            .lines()
            .filter_map(|l| l.strip_prefix("@@ "))
            .filter_map(|l| l.split_once('='))
            .collect();
        ran += 1;
        if report["first"] != report["second"] {
            failures.push(format!(
                "{}: last frame differs ({} vs {})",
                case.core, report["first"], report["second"]
            ));
        }
        if case.resident && report["resident"] != "1" {
            failures.push(format!("{}: no private copy journalled", case.core));
        }
        if report["leftovers"] != "0" {
            failures.push(format!("{}: copy files left behind", case.core));
        }
        eprintln!(
            "OK {} (resident={}, frame {})",
            case.core, report["resident"], report["first"]
        );
    }
    assert!(ran > 0, "no core present");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
