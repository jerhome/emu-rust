//! `emu-rust` binary. Phase 1 CLI (see docs/design/phase1-poc.md):
//! `emu-rust --core <core.so> --rom <file> [--scale N] [--frames N] [--dump-frame out.png]
//!           [--system-dir DIR] [--save-dir DIR]`

mod cli;

use std::process::ExitCode;

fn main() -> ExitCode {
    let options = match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli::Command::Run(o)) => o,
        Ok(cli::Command::Help) => {
            println!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("erreur : {e}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    for dir in [&options.system_dir, &options.save_dir] {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("erreur : impossible de créer {} : {e}", dir.display());
            return ExitCode::FAILURE;
        }
    }
    match emu_ui::run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::FAILURE
        }
    }
}
