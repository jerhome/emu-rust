//! ADR 0022 on the mock core, not ignored (Linux): the library is linked with `-z nodelete`, so `dlclose` leaves it
//! mapped, and `MOCK_REFUSE_REINIT` makes a second `retro_init` in the same image abort. Loading it twice in a
//! process therefore only works through the private copy. Runs in a child process (this binary re-executed) so
//! that a regression is a named failure, not the death of the test binary.
#![cfg(target_os = "linux")]

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use emu_libretro::{Core, CoreConfig, FrontendHandler, InputQuery, LibretroError, VideoFrame};

#[derive(Default)]
struct Probe {
    frames: u32,
    last: Vec<u8>,
}

impl FrontendHandler for Probe {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
        if let Some(f) = frame {
            self.frames += 1;
            self.last = f.data.to_vec();
        }
    }
    fn audio_samples(&mut self, _samples: &[i16]) {}
    fn input_poll(&mut self) {}
    fn input_state(&mut self, _query: InputQuery) -> i16 {
        0
    }
}

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

fn config(dir: &Path, copy_dir: Option<&Path>) -> CoreConfig {
    CoreConfig {
        system_dir: dir.join("system"),
        save_dir: dir.join("saves"),
        assets_dir: dir.join("assets"),
        variables: Vec::new(),
        provide_vfs: false,
        copy_dir: copy_dir.map(Path::to_path_buf),
    }
}

fn play(lib: &Path, rom: &Path, config: CoreConfig) -> Result<Vec<u8>, LibretroError> {
    let mut core = Core::load(lib, config)?;
    core.load_game(rom).expect("load game");
    let mut probe = Probe::default();
    for _ in 0..30 {
        core.run(&mut probe).expect("run");
    }
    assert_eq!(probe.frames, 30);
    Ok(probe.last)
}

/// Child process: three loads of the same library.
#[test]
fn zz_worker() {
    let Some(dir) = std::env::var_os("RELOAD_MOCK_DIR") else {
        return;
    };
    let dir = Path::new(&dir);
    let journal = Journal::default();
    tracing::subscriber::set_global_default(journal.clone()).unwrap();
    let lib = mock_core::build(mock_core::Variant::NoDelete);
    let rom = dir.join("game.mock");
    std::fs::write(&rom, b"mock content").unwrap();
    let copies = dir.join("copies");

    let first = play(&lib, &rom, config(dir, Some(&copies))).expect("first load");
    assert!(
        journal
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("stayed in memory")),
        "the probe must see the library resident"
    );
    // Second and third loads: the original is resident, the guard would abort without a private copy.
    let second = play(&lib, &rom, config(dir, Some(&copies))).expect("second load (private copy)");
    let third = play(&lib, &rom, config(dir, Some(&copies))).expect("third load (private copy)");
    assert_eq!(first, second, "same image as a fresh process");
    assert_eq!(first, third);
    assert_eq!(
        std::fs::read_dir(&copies).map_or(0, Iterator::count),
        0,
        "copies are unlinked right after dlopen"
    );

    // Without a copy directory the reload must fail cleanly instead of reusing the resident image.
    match Core::load(&lib, config(dir, None)) {
        Err(LibretroError::CoreCopy { .. }) => {}
        other => panic!("expected CoreCopy, got {:?}", other.map(|_| ())),
    }
    println!("@@ done");
}

#[test]
fn resident_mock_core_reloads_through_a_private_copy() {
    let dir = mock_core::output_dir().join("reload-mock");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "zz_worker", "--nocapture", "--test-threads=1"])
        .env("RELOAD_MOCK_DIR", &dir)
        .env("MOCK_REFUSE_REINIT", "1")
        .output()
        .expect("spawn the worker");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success() && stdout.contains("@@ done"),
        "worker failed ({}):\n{stdout}\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
}
