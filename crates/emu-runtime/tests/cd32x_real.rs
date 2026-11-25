//! Real Sega CD 32X test (7.2, design v7.2 §4): picodrive loads a generated disc tagged `(Sega CD 32X)` with the
//! synthetic BIOS of `mkcd.py bios-stub`, and logs `32X startup`. It proves that the 32X is switched on by
//! picodrive inside the emu-rust pipeline, not the routing and not a game. The BIOS is written in a temporary
//! folder only (never committed, never in a user's `system/`).
//!
//! `#[ignore]`: needs `tests/cores/picodrive_libretro.so` (`scripts/fetch-test-assets.sh --with-6.0`) and `python3`.
//! Run: `cargo test -p emu-runtime --test cd32x_real -- --ignored --nocapture`. Without the core the test returns
//! with a message.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use emu_runtime::{CollectSink, ContentSpec, Emulator, EmulatorOptions, NullInput, SaveScope};

/// Messages of the core (`emu_libretro::core` target) and of the loader, in order.
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

struct Spy;

impl tracing::Subscriber for Spy {
    fn enabled(&self, m: &tracing::Metadata<'_>) -> bool {
        *m.level() <= tracing::Level::DEBUG
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        let mut m = Message(String::new());
        e.record(&mut m);
        LOG.lock().unwrap_or_else(|p| p.into_inner()).push(m.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mkcd(args: &[&str]) {
    let out = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools/mkcd.py"))
        .args(args)
        .output()
        .expect("python3 is required for the fixtures");
    assert!(
        out.status.success(),
        "mkcd.py {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn log_has(needle: &str) -> bool {
    LOG.lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|l| l.contains(needle))
}

#[test]
#[ignore = "needs picodrive: scripts/fetch-test-assets.sh --with-6.0"]
fn picodrive_starts_the_32x_on_a_tagged_disc_with_the_stub_bios() {
    let core = root().join("tests/cores/picodrive_libretro.so");
    if !core.is_file() {
        eprintln!(
            "SKIPPED: {} missing (scripts/fetch-test-assets.sh --with-6.0)",
            core.display()
        );
        return;
    }
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        tracing::subscriber::set_global_default(Spy).expect("no other global subscriber");
    });

    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    for sub in ["system", "saves", "states", "assets", "disc"] {
        fs::create_dir_all(data.join(sub)).unwrap();
    }
    let disc = data.join("disc");
    mkcd(&[
        "megacd",
        disc.to_str().unwrap(),
        "--name",
        "Fake Trap (USA) (Sega CD 32X)",
        "--variant",
        "32x",
        "--region",
        "U",
    ]);
    // Temporary system folder only; picodrive looks for the region BIOS of the disc.
    mkcd(&[
        "bios-stub",
        data.join("system/bios_CD_U.bin").to_str().unwrap(),
    ]);

    let options = EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        cache_dir: data.join("cache"),
        provide_vfs: false,
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: Vec::new(),
        restore_added_disks: false,
        core_save_scope: SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
        port_devices: Vec::new(),
    };
    let cue = disc.join("Fake Trap (USA) (Sega CD 32X).cue");
    let mut emu = Emulator::load_content(&core, ContentSpec::File(cue), options)
        .unwrap_or_else(|e| panic!("picodrive refused the disc with the stub BIOS: {e}"));
    assert!(emu.system_info().library_name.contains("PicoDrive"));
    let mut sink = CollectSink::default();
    for _ in 0..30 {
        emu.run_frame(&mut NullInput, &mut sink).expect("run_frame");
    }
    drop(emu);
    let lines = LOG.lock().unwrap_or_else(|p| p.into_inner()).clone();
    assert!(
        log_has("32X startup"),
        "no `32X startup` in the core log; {} lines, last ones: {:?}",
        lines.len(),
        lines.iter().rev().take(15).collect::<Vec<_>>()
    );
}
