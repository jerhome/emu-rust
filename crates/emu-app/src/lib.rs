//! The `emu-rust` program: assembles CLI, data directories (ADR 0011) and `config.toml`, then runs the UI.
//! A library (ADR 0023 §3): `emu-rust` and, on Windows, `emu-rust-cli` are thin binaries calling [`run`].

mod cli;
mod cores_cli;
mod diagnostics;
mod logging;
#[cfg(any(test, target_os = "macos"))]
mod terminal_macos;
mod text;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

/// Exit code of a command-line usage error (unknown option, missing value...), in both flavors.
const USAGE_ERROR_CODE: u8 = 2;

/// Variable set on the `emu-rust-cli.exe` relaunched by `emu-rust.exe` for `[app] show_terminal = true`: the
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
/// no standard output (not redirected) + « Afficher le terminal » (`[app] show_terminal`, alias
/// `[interface] console`) + not already a relaunched child.
pub fn console_plan(
    flavor: Flavor,
    has_stdout: bool,
    window_mode: bool,
    wanted: bool,
    is_child: bool,
) -> ConsolePlan {
    if flavor == Flavor::Gui && !has_stdout && window_mode && wanted && !is_child {
        ConsolePlan::Relaunch
    } else {
        ConsolePlan::Stay
    }
}

/// `true` when standard output leads somewhere (console, pipe, file). A graphical-subsystem Windows process
/// started from the Explorer has a null or invalid handle; everywhere else the handle always exists.
fn has_stdout() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle as _;
        let handle = std::io::stdout().as_raw_handle();
        !handle.is_null() && handle as isize != -1
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// The console executable next to this one.
fn console_sibling() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let sibling = exe.with_file_name(format!("emu-rust-cli{}", std::env::consts::EXE_SUFFIX));
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Err(format!("{} est introuvable", sibling.display()))
    }
}

/// Starts `emu-rust-cli` with the same arguments; Windows gives it a console. The caller then exits.
fn relaunch_with_console() -> Result<(), String> {
    let sibling = console_sibling()?;
    std::process::Command::new(sibling)
        .args(std::env::args_os().skip(1))
        .env(CONSOLE_CHILD_ENV, "1")
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

/// What a graphical executable without standard output says to a command-line use.
fn cli_refused(flavor: Flavor, code: ExitCode) -> ExitCode {
    if flavor == Flavor::Gui && !has_stdout() {
        emu_ui::fatal_dialog(text::TITLE, text::USE_CLI_EXECUTABLE);
    }
    code
}

fn standard_root() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "emu-rust").map(|d| d.data_dir().to_path_buf())
}

/// Data root of a command, before tracing is set up (log file, `[app] show_terminal`); `None` without one.
fn early_root(command: &cli::Command) -> Option<PathBuf> {
    let data_dir = match command {
        cli::Command::Cores(c) => c.data_dir.clone(),
        cli::Command::Library(l) => l.data_dir.clone(),
        cli::Command::Run(c) => c.data_dir.clone(),
        cli::Command::Help | cli::Command::Version => return None,
    };
    cli::resolve_root(data_dir, std::env::var_os("EMU_DATA_DIR"), standard_root()).ok()
}

/// Data directories (overrides applied, folders created) and `config.toml`.
fn prepare(
    data_dir: Option<PathBuf>,
    system_dir: Option<PathBuf>,
    save_dir: Option<PathBuf>,
) -> Result<(emu_ui::DataDirs, emu_ui::Config), String> {
    let root = cli::resolve_root(data_dir, std::env::var_os("EMU_DATA_DIR"), standard_root())?;
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
    // The game's path may be one an interrupted ROM tidying moved (ADR 0023 §4.5); a capture never moves files.
    if cli.dump_window.is_none() {
        emu_ui::finish_pending_identities(&dirs);
    }
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

/// Fatal error: journal (hence the log file), stderr, and a native box when nobody can read stderr.
fn fail(flavor: Flavor, message: &str) -> ExitCode {
    // The tracing stderr layer already prints it when it is active: print once.
    tracing::error!("{message}");
    if !logging::stderr_layer_active() {
        eprintln!("erreur : {message}");
    }
    if flavor == Flavor::Gui && !has_stdout() {
        emu_ui::fatal_dialog(text::TITLE, message);
    }
    ExitCode::FAILURE
}

fn report(flavor: Flavor, result: Result<(), emu_ui::UiError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let mut message = e.to_string();
            if matches!(e, emu_ui::UiError::Runtime(_)) {
                message = format!("{message}\n{}", diagnostics::failure_hint(e.core_error()));
            }
            fail(flavor, &message)
        }
    }
}

/// « Afficher le terminal » before anything else: Windows relaunches `emu-rust-cli.exe` for a window mode, macOS
/// restarts inside Terminal.app. `Ok(true)`: this process must exit. `Err`: message to log once tracing is up.
fn relaunch_for_terminal(
    flavor: Flavor,
    command: Option<&cli::Command>,
    args: &[OsString],
    show_terminal: bool,
) -> Result<bool, String> {
    let window_mode = matches!(
        command,
        Some(cli::Command::Library(_) | cli::Command::Run(_))
    );
    let plan = console_plan(
        flavor,
        has_stdout(),
        window_mode,
        show_terminal,
        std::env::var_os(CONSOLE_CHILD_ENV).is_some(),
    );
    if plan == ConsolePlan::Relaunch {
        return relaunch_with_console()
            .map(|()| true)
            .map_err(|e| format!("console non ouverte, démarrage sans console : {e}"));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = args;
    // Only an argument-less launch (Finder, Dock) may move to Terminal.app: `open` cannot forward arguments.
    #[cfg(target_os = "macos")]
    if terminal_macos::is_restart_candidate(command, args) {
        return terminal_macos::restart_in_terminal(show_terminal);
    }
    Ok(false)
}

/// The whole program; `flavor` says which executable runs it.
pub fn run(flavor: Flavor) -> ExitCode {
    emu_ui::log::install_panic_hook();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let parsed = cli::parse(args.clone());
    let root = parsed.as_ref().ok().and_then(early_root);
    // Read again by `prepare`; a broken file is reported there, once tracing is up. The 7.1.0-rc.1 key
    // `[interface] console` is read as `show_terminal` by `Config::load`.
    let show_terminal = root.as_deref().is_some_and(|r| {
        emu_ui::Config::load_or_default(&emu_ui::DataDirs::under(r).config_file)
            .app
            .show_terminal
    });
    let relaunch = relaunch_for_terminal(flavor, parsed.as_ref().ok(), &args, show_terminal);
    if relaunch == Ok(true) {
        return ExitCode::SUCCESS;
    }
    // Only the window modes own the log file (and its rotation): a CLI command must not push the trace of a
    // crash to `.1` and then overwrite it.
    let log_root = root
        .as_deref()
        .filter(|_| logging::owns_log_file(parsed.as_ref().ok()));
    logging::init_tracing(log_root, show_terminal);
    if let Err(e) = relaunch {
        tracing::warn!("{e}");
    }
    match parsed {
        Ok(cli::Command::Help) => {
            println!("{}", cli::USAGE);
            cli_refused(flavor, ExitCode::SUCCESS)
        }
        Ok(cli::Command::Version) => {
            println!("{}", cli::version_line());
            cli_refused(flavor, ExitCode::SUCCESS)
        }
        Ok(cli::Command::Cores(c)) => {
            if flavor == Flavor::Gui && !has_stdout() {
                // Unlike --help/--version (informational, SUCCESS), the requested action was not run: FAILURE.
                return cli_refused(flavor, ExitCode::FAILURE);
            }
            match prepare(c.data_dir, c.system_dir, None)
                .and_then(|(dirs, config)| cores_cli::run(&c.action, &dirs, &config))
            {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::FAILURE,
                Err(e) => fail(flavor, &e),
            }
        }
        Ok(cli::Command::Library(l)) => match prepare(l.data_dir, l.system_dir, l.save_dir) {
            Ok((dirs, config)) => {
                // An interrupted ROM tidying is finished before the library opens (ADR 0023 §4.5); a capture
                // (`--dump-window`) never moves the user's files.
                if l.dump_window.is_none() {
                    emu_ui::finish_pending_identities(&dirs);
                }
                install_signal_handler();
                report(
                    flavor,
                    emu_ui::run_library(emu_ui::LibraryOptions {
                        dirs,
                        config,
                        audio: l.audio,
                        gamepad: l.gamepad,
                        screen: l.screen,
                        dump_window: l.dump_window,
                        wizard: l.wizard,
                    }),
                )
            }
            Err(e) => fail(flavor, &e),
        },
        Ok(cli::Command::Run(c)) => match build_options(*c) {
            Ok(options) => {
                install_signal_handler();
                report(flavor, emu_ui::run(options))
            }
            Err(e) => fail(flavor, &e),
        },
        Err(e) => {
            eprintln!("erreur : {e}\n\n{}", cli::USAGE);
            cli_refused(flavor, ExitCode::from(USAGE_ERROR_CODE))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every combination (ADR 0023 §3): a relaunch needs all five conditions.
    #[test]
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

    #[test]
    fn unknown_option_is_a_usage_error_with_code_2() {
        assert!(cli::parse(["--no-such-option"].map(OsString::from)).is_err());
        assert_eq!(USAGE_ERROR_CODE, 2);
        // Console flavor, and a Gui one with a stdout (tests): the code goes through untouched.
        for flavor in [Flavor::Console, Flavor::Gui] {
            assert_eq!(
                format!(
                    "{:?}",
                    cli_refused(flavor, ExitCode::from(USAGE_ERROR_CODE))
                ),
                format!("{:?}", ExitCode::from(2))
            );
        }
    }

    #[test]
    fn subcommands_never_relaunch() {
        let cores = cli::parse(["--list-cores"].map(std::ffi::OsString::from)).unwrap();
        // Console flavor or a stdout present (tests): never a relaunch, whatever the setting.
        assert_eq!(
            relaunch_for_terminal(
                Flavor::Console,
                Some(&cores),
                &[OsString::from("--list-cores")],
                true
            ),
            Ok(false)
        );
    }
}
