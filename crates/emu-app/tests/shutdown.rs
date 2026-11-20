// The mock core and the signals only exist on Unix.
#![cfg(unix)]
//! SIGTERM / SIGINT end a running game through the normal path (2.1, Mega CD `.brm`): the process exits with
//! success, logs « jeu fermé, sauvegardes écrites », and the SRAM changed by the game is on disk.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use mock_core::Variant;

fn root() -> PathBuf {
    mock_core::workspace_root()
}

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

fn app_binary() -> PathBuf {
    // Cargo builds the binary of this package before its integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_emu-rust"))
}

fn out_dir(name: &str) -> PathBuf {
    let dir = mock_core::output_dir().join("shutdown").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn inject(display: &str, actions: &[&str]) {
    let status = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools/xinject.py"))
        .arg(display)
        .args(actions)
        .status()
        .expect("python3");
    assert!(status.success(), "xinject failed");
}

fn signal(child: &Child, name: &str) {
    let status = Command::new("kill")
        .arg(format!("-{name}"))
        .arg(child.id().to_string())
        .status()
        .expect("kill");
    assert!(status.success());
}

/// Waits for the exit; returns (success, stderr).
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

fn srm_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(srm_files(&p));
        } else if p.extension().is_some_and(|x| x == "srm") {
            out.push(p);
        }
    }
    out
}

fn run_and_signal(name: &str, sig: &str) {
    let dir = out_dir(name);
    let data = dir.join("data");
    let rom = dir.join("game.bin");
    fs::write(&rom, b"shutdown test rom").unwrap();
    let core = mock_core::build(Variant::Default);
    let x = Xvfb::start(&dir);
    let child = Command::new(app_binary())
        .env_remove("WAYLAND_DISPLAY")
        .env("DISPLAY", &x.display)
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("RUST_LOG", "info")
        .arg("--core")
        .arg(&core)
        .arg("--rom")
        .arg(&rom)
        .arg("--data-dir")
        .arg(&data)
        .args(["--no-audio", "--no-gamepad"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("emu-rust");
    std::thread::sleep(Duration::from_secs(2));
    // The mock core writes its frame counter to SRAM while Start (Enter) is held.
    inject(
        &x.display,
        &["focus", "down", "Return", "sleep", "0.5", "up", "Return"],
    );
    signal(&child, sig);
    let (ok, log) = wait_exit(child, Duration::from_secs(20));
    assert!(ok, "emu-rust failed after SIG{sig}:\n{log}");
    assert!(
        log.contains("jeu fermé, sauvegardes écrites"),
        "no shutdown log:\n{log}"
    );
    let saves = srm_files(&data.join("saves"));
    assert_eq!(saves.len(), 1, "SRAM not written:\n{log}");
    let sram = fs::read(&saves[0]).unwrap();
    assert_eq!(sram.get(4), Some(&0xA5), "SRAM content not from the game");
}

#[test]
#[ignore = "needs Xvfb, libXtst and python3"]
fn sigterm_writes_sram_and_exits_cleanly() {
    run_and_signal("sigterm", "TERM");
}

#[test]
#[ignore = "needs Xvfb, libXtst and python3"]
fn sigint_writes_sram_and_exits_cleanly() {
    run_and_signal("sigint", "INT");
}
