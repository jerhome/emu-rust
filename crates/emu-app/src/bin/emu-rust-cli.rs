//! `emu-rust-cli.exe` (feature `cli-bin`, Windows packages): the same program in the console subsystem, waited for
//! by `cmd` / PowerShell with its exit code (ADR 0023 §3).

fn main() -> std::process::ExitCode {
    emu_app::run(emu_app::Flavor::Console)
}
