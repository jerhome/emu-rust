//! `emu-rust`: on Windows the graphical-subsystem executable (no terminal on double-click, ADR 0023 §3); elsewhere
//! the only binary, a console program as before.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    emu_app::run(if cfg!(windows) {
        emu_app::Flavor::Gui
    } else {
        emu_app::Flavor::Console
    })
}
