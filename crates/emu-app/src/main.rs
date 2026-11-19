//! `emu-rust` binary: assembles CLI, data directories (ADR 0011) and `config.toml`, then runs the UI.

mod cli;

use std::process::ExitCode;

const DEFAULT_LOG_FILTER: &str = "warn,emu_app=info,emu_ui=info,emu_runtime=info,emu_libretro=info";

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
}

fn build_options(cli: cli::Cli) -> Result<emu_ui::LaunchOptions, String> {
    let standard =
        directories::ProjectDirs::from("", "", "emu-rust").map(|d| d.data_dir().to_path_buf());
    let root = cli::resolve_root(cli.data_dir, std::env::var_os("EMU_DATA_DIR"), standard)?;
    tracing::info!("racine des données : {}", root.display());
    let mut dirs = emu_ui::DataDirs::under(&root);
    if let Some(d) = cli.system_dir {
        dirs.system =
            std::path::absolute(&d).map_err(|e| format!("--system-dir {} : {e}", d.display()))?;
    }
    if let Some(d) = cli.save_dir {
        dirs.saves =
            std::path::absolute(&d).map_err(|e| format!("--save-dir {} : {e}", d.display()))?;
    }
    dirs.create_all()
        .map_err(|e| format!("impossible de créer {} : {e}", dirs.root.display()))?;
    let config = emu_ui::Config::load_or_default(&dirs.config_file);
    Ok(emu_ui::LaunchOptions {
        core: cli.core,
        rom: cli.rom,
        dirs,
        config,
        scale: cli.scale,
        audio: cli.audio,
        exit_after_frames: cli.frames,
        dump_last_frame: cli.dump_frame,
        screen: cli.screen,
        dump_window: cli.dump_window,
    })
}

fn main() -> ExitCode {
    init_tracing();
    let cli = match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli::Command::Run(c)) => c,
        Ok(cli::Command::Help) => {
            println!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("erreur : {e}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    let options = match build_options(*cli) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("erreur : {e}");
            return ExitCode::FAILURE;
        }
    };
    match emu_ui::run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::FAILURE
        }
    }
}
