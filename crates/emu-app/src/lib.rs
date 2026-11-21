//! The `emu-rust` program: assembles CLI, data directories (ADR 0011) and `config.toml`, then runs the UI.
//! A library since 7.1 (ADR 0023 §3): `emu-rust` and, on Windows, `emu-rust-cli` are thin binaries calling [`run`].

mod cli;
mod cores_cli;
mod diagnostics;

use std::process::ExitCode;

use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

const DEFAULT_LOG_FILTER: &str = "warn,emu_app=info,emu_ui=info,emu_runtime=info,emu_libretro=info";

/// Variable set on the `emu-rust-cli.exe` relaunched by `emu-rust.exe` for `[interface] console = true`: the
/// child never relaunches itself (ADR 0023 §3).
pub const CONSOLE_CHILD_ENV: &str = "EMU_RUST_CONSOLE_CHILD";

/// Which executable runs the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// `emu-rust.exe` on Windows: graphical subsystem, no console of its own.
    Gui,
    /// `emu-rust-cli.exe` on Windows, and the only `emu-rust` binary on Linux / macOS.
    Console,
}

/// Outcome of [`console_plan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsolePlan {
    /// Run in this process.
    Stay,
    /// Spawn the sibling `emu-rust-cli.exe` with the same arguments and [`CONSOLE_CHILD_ENV`]`=1`, then exit 0
    /// (Windows gives the child a console). A failed spawn is logged and the program stays.
    Relaunch,
}

/// Pure decision (ADR 0023 §3): `Relaunch` only for `Gui` + window mode (library or game, not a subcommand) +
/// no standard output (not redirected) + `[interface] console = true` + not already a relaunched child.
pub fn console_plan(
    flavor: Flavor,
    has_stdout: bool,
    window_mode: bool,
    wanted: bool,
    is_child: bool,
) -> ConsolePlan {
    let _ = (flavor, has_stdout, window_mode, wanted, is_child);
    todo!("FU1: ADR 0023 §3 console decision")
}

/// stderr output as before, plus the capture layer feeding the « Journal » panel (`emu_ui::log`).
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false),
        )
        .with(emu_ui::log::capture_layer())
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
    let content = match cli.roms.as_slice() {
        [] => emu_ui::GameContent::NoGame,
        // DOS: a game folder (snapshot built here, with progress) or `JEU/START.EXE`; `.conf` refused.
        [one] => match emu_ui::dos_cli_content(one, &core, &config) {
            Some(content) => {
                let content = content?;
                let mut last = None;
                let built = emu_ui::prepare_dos_snapshot(
                    &core,
                    &content,
                    &dirs,
                    &config,
                    &mut |done, total| {
                        let percent = done.saturating_mul(100).checked_div(total).unwrap_or(100);
                        if last != Some(percent) {
                            last = Some(percent);
                            eprint!("\rPréparation du jeu : {percent} %   ");
                        }
                    },
                )?;
                if built {
                    eprintln!();
                }
                content
            }
            None => emu_ui::GameContent::Files(cli.roms),
        },
        _ => emu_ui::GameContent::Files(cli.roms),
    };
    Ok(emu_ui::LaunchOptions {
        core,
        content,
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
        launch: cli.launch,
        auto_fast_load: cli.auto_fast_load,
        pad_device: cli.pad_device,
        game_system: None,
        region: None,
        spec: None,
    })
}

/// SIGINT / SIGTERM / SIGHUP (Ctrl+C, `kill`, closed terminal; console events on Windows): the window quits
/// through its normal path, so the game is unloaded and its saves are written. A second signal forces the exit.
/// Installed only for the window modes: the CLI subcommands keep the default behaviour.
fn install_signal_handler() {
    let result = ctrlc::set_handler(|| {
        if emu_ui::request_shutdown() {
            eprintln!("arrêt forcé");
            std::process::exit(130);
        }
        eprintln!(
            "arrêt demandé : fermeture propre et écriture des sauvegardes (Ctrl+C de nouveau pour forcer)"
        );
    });
    if let Err(e) = result {
        tracing::warn!("gestionnaire d'arrêt non installé : {e}");
    }
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
                eprintln!("{}", diagnostics::failure_hint(e.core_error()));
            }
            ExitCode::FAILURE
        }
    }
}

/// The whole program; `flavor` says which executable runs it. Until FU1, both flavors behave as the 7.0 binary
/// (no relaunch, no dialog).
pub fn run(flavor: Flavor) -> ExitCode {
    let _ = flavor;
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
            Ok((dirs, config)) => {
                install_signal_handler();
                report(emu_ui::run_library(emu_ui::LibraryOptions {
                    dirs,
                    config,
                    audio: l.audio,
                    gamepad: l.gamepad,
                    screen: l.screen,
                    dump_window: l.dump_window,
                }))
            }
            Err(e) => fail(&e),
        },
        Ok(cli::Command::Run(c)) => match build_options(*c) {
            Ok(options) => {
                install_signal_handler();
                report(emu_ui::run(options))
            }
            Err(e) => fail(&e),
        },
        Err(e) => {
            eprintln!("erreur : {e}\n\n{}", cli::USAGE);
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every combination (design v7.1 §5, FU1): a relaunch needs all five conditions.
    #[test]
    #[ignore = "FU1: console_plan is a todo!() until then"]
    fn console_plan_relaunches_only_a_windowed_gui_without_stdout() {
        for flavor in [Flavor::Gui, Flavor::Console] {
            for bits in 0u8..16 {
                let [has_stdout, window_mode, wanted, is_child] =
                    [0, 1, 2, 3].map(|i| bits & (1 << i) != 0);
                let expected =
                    if flavor == Flavor::Gui && !has_stdout && window_mode && wanted && !is_child {
                        ConsolePlan::Relaunch
                    } else {
                        ConsolePlan::Stay
                    };
                assert_eq!(
                    console_plan(flavor, has_stdout, window_mode, wanted, is_child),
                    expected,
                    "{flavor:?} stdout={has_stdout} window={window_mode} wanted={wanted} child={is_child}"
                );
            }
        }
    }
}
