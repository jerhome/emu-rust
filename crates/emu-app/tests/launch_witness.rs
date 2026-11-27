// The mock core and the signals only exist on Unix.
#![cfg(unix)]
//! The launch witness for real (ADR 0025 §5): a game that ends cleanly leaves no marker; a core that aborts
//! leaves one, and the next start reports it once and removes it.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use mock_core::Variant;

struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    fn start(dir: &Path) -> Self {
        let tmp = dir.join("xvfb-display");
        let _ = fs::remove_file(&tmp);
        let child = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "exec Xvfb -displayfd 3 -screen 0 1280x720x24 -nolisten tcp 3>{} 2>/dev/null",
                tmp.display()
            ))
            .spawn()
            .expect("Xvfb is needed");
        let start = Instant::now();
        let display = loop {
            if let Ok(text) = fs::read_to_string(&tmp)
                && let Some(line) = text.lines().next().filter(|l| !l.is_empty())
            {
                break format!(":{line}");
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "Xvfb did not start"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        Xvfb { child, display }
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn out_dir(name: &str) -> PathBuf {
    let dir = mock_core::output_dir().join("launch-witness").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Starts a game with the mock core (`env` for the core) and returns the child.
fn launch(x: &Xvfb, data: &Path, rom: &Path, env: &[(&str, &str)]) -> Child {
    let core = mock_core::build(Variant::Default);
    Command::new(env!("CARGO_BIN_EXE_emu-rust"))
        .env_remove("WAYLAND_DISPLAY")
        .env("DISPLAY", &x.display)
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("RUST_LOG", "info")
        .envs(env.iter().copied())
        .arg("--core")
        .arg(&core)
        .arg("--rom")
        .arg(rom)
        .arg("--data-dir")
        .arg(data)
        .args(["--no-audio", "--no-gamepad"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("emu-rust")
}

fn wait_exit(mut child: Child, limit: Duration) -> (bool, String) {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_string(&mut err);
            }
            return (status.success(), err);
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            panic!("emu-rust did not exit within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn markers(data: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(data.join("cache")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("launch-"))
        })
        .collect()
}

fn terminate(child: &Child) {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .expect("kill");
    assert!(status.success());
}

#[test]
#[ignore = "needs Xvfb"]
fn a_crash_leaves_a_marker_that_the_next_start_reports_once() {
    let dir = out_dir("crash");
    let data = dir.join("data");
    let rom = dir.join("game.bin");
    fs::write(&rom, b"witness test rom").unwrap();
    let x = Xvfb::start(&dir);

    // The core aborts a few frames in: the marker of the launch stays.
    let (ok, _) = wait_exit(
        launch(&x, &data, &rom, &[("MOCK_CRASH_AT_FRAME", "5")]),
        Duration::from_secs(30),
    );
    assert!(!ok, "the core was meant to abort");
    let left = markers(&data);
    assert_eq!(left.len(), 1, "one marker left: {left:?}");
    let text = fs::read_to_string(&left[0]).unwrap();
    assert!(
        text.contains("game.bin"),
        "the marker names the game: {text}"
    );
    assert!(
        !text.contains(dir.to_str().unwrap()),
        "no full path: {text}"
    );

    // The next start reports it, removes it, and a clean end leaves nothing.
    let child = launch(&x, &data, &rom, &[]);
    std::thread::sleep(Duration::from_secs(3));
    terminate(&child);
    let (ok, log) = wait_exit(child, Duration::from_secs(20));
    assert!(ok, "second run failed:\n{log}");
    assert!(
        log.contains("la session précédente s'est arrêtée pendant game.bin"),
        "the stop was not reported:\n{log}"
    );
    assert!(markers(&data).is_empty(), "markers: {:?}", markers(&data));
}

#[test]
#[ignore = "needs Xvfb"]
fn a_clean_end_leaves_no_marker() {
    let dir = out_dir("clean");
    let data = dir.join("data");
    let rom = dir.join("game.bin");
    fs::write(&rom, b"witness test rom").unwrap();
    let x = Xvfb::start(&dir);
    let child = launch(&x, &data, &rom, &[]);
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(markers(&data).len(), 1, "a marker while the game runs");
    terminate(&child);
    let (ok, log) = wait_exit(child, Duration::from_secs(20));
    assert!(ok, "run failed:\n{log}");
    assert!(markers(&data).is_empty(), "markers: {:?}", markers(&data));
}
