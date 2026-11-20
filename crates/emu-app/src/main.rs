//! `emu-rust` binary: assembles CLI, data directories (ADR 0011) and `config.toml`, then runs the UI.

mod cli;
mod cores_cli;
mod diagnostics;

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

/// Data directories (overrides applied, folders created) and `config.toml`.
fn prepare(
    data_dir: Option<std::path::PathBuf>,
    system_dir: Option<std::path::PathBuf>,
    save_dir: Option<std::path::PathBuf>,
) -> Result<(emu_ui::DataDirs, emu_ui::Config), String> {
    let standard =
        directories::ProjectDirs::from("", "", "emu-rust").map(|d| d.data_dir().to_path_buf());
    let root = cli::resolve_root(data_dir, std::env::var_os("EMU_DATA_DIR"), standard)?;
    tracing::info!("racine des données : {}", root.display());
    let mut dirs = emu_ui::DataDirs::under(&root);
    if let Some(d) = system_dir {
        dirs.system =
            std::path::absolute(&d).map_err(|e| format!("--system-dir {} : {e}", d.display()))?;
    }
    if let Some(d) = save_dir {
        dirs.saves =
            std::path::absolute(&d).map_err(|e| format!("--save-dir {} : {e}", d.display()))?;
    }
    dirs.create_all()
        .map_err(|e| format!("impossible de créer {} : {e}", dirs.root.display()))?;
    let config = emu_ui::Config::load_or_default(&dirs.config_file);
    Ok((dirs, config))
}

fn build_options(cli: cli::Cli) -> Result<emu_ui::LaunchOptions, String> {
    let (dirs, config) = prepare(cli.data_dir, cli.system_dir, cli.save_dir)?;
    let core = match cli.core {
        cli::CoreSpec::Path(path) => path,
        cli::CoreSpec::Managed(id) => cores_cli::managed_core_path(&id, &dirs, &config)?,
    };
    diagnostics::check_core(&core)?;
    Ok(emu_ui::LaunchOptions {
        core,
        content: if cli.roms.is_empty() {
            emu_ui::GameContent::NoGame
        } else {
            emu_ui::GameContent::Files(cli.roms)
        },
        // Computer cores (hatari…) are recognised by the UI from their id.
        computer: false,
        dirs,
        config,
        scale: cli.scale,
        audio: cli.audio,
        gamepad: cli.gamepad,
        exit_after_frames: cli.frames,
        dump_last_frame: cli.dump_frame,
        screen: cli.screen,
        dump_window: cli.dump_window,
    })
}

fn fail(message: &str) -> ExitCode {
    eprintln!("erreur : {message}");
    ExitCode::FAILURE
}

fn report(result: Result<(), emu_ui::UiError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e}");
            if matches!(e, emu_ui::UiError::Runtime(_)) {
                eprintln!("{}", diagnostics::load_failure_hint());
            }
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    init_tracing();
    match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli::Command::Help) => {
            println!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Version) => {
            println!("{}", cli::version_line());
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Cores(c)) => match prepare(c.data_dir, c.system_dir, None)
            .and_then(|(dirs, config)| cores_cli::run(&c.action, &dirs, &config))
        {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(e) => fail(&e),
        },
        Ok(cli::Command::Library(l)) => match prepare(l.data_dir, l.system_dir, l.save_dir) {
            Ok((dirs, config)) => report(emu_ui::run_library(emu_ui::LibraryOptions {
                dirs,
                config,
                audio: l.audio,
                gamepad: l.gamepad,
                screen: l.screen,
                dump_window: l.dump_window,
            })),
            Err(e) => fail(&e),
        },
        Ok(cli::Command::Run(c)) => match build_options(*c) {
            Ok(options) => report(emu_ui::run(options)),
            Err(e) => fail(&e),
        },
        Err(e) => {
            eprintln!("erreur : {e}\n\n{}", cli::USAGE);
            ExitCode::from(2)
        }
    }
}
