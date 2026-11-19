//! Hand-written argument parser (no `clap` for a few dozen options). Messages are user-facing French.
//! Pure: data directories and `config.toml` are resolved afterwards by `main`. Core ids are only checked
//! syntactically here; `emu_assets::cores::CoreId` validates them when the command runs.

use std::ffi::OsString;
use std::path::PathBuf;

use emu_ui::{LibraryScreen, MAX_SCALE, MIN_SCALE, Screen};

pub const USAGE: &str = "Usage : emu-rust                         ouvre la bibliothèque de jeux\n\
                         \x20      emu-rust --core <ID|chemin> --rom <fichier> [--data-dir DIR] [--system-dir DIR]\n\
                         \x20                [--save-dir DIR] [--scale N] [--no-audio] [--frames N]\n\
                         \x20                [--dump-frame sortie.png] [--screen menu|keys|options]\n\
                         \x20                [--dump-window sortie.png]\n\
                         \x20      emu-rust [--data-dir DIR] [--no-audio] [--screen library|library-settings|cores|library-launch]\n\
                         \x20                [--dump-window sortie.png]\n\
                         \x20      emu-rust [--data-dir DIR] --list-cores | --refresh-cores | --update-cores\n\
                         \x20      emu-rust [--data-dir DIR] --install-core <ID|recommended> [--install-core ...]\n\
                         \x20      emu-rust [--data-dir DIR] --remove-core <ID> | --check-bios [ID]\n\
                         \x20      emu-rust --version | --help\n\
                         --core accepte l'identifiant d'un core installé (ex. gambatte) ou le chemin d'un fichier.\n\
                         Racine de données : --data-dir, sinon EMU_DATA_DIR, sinon le dossier standard.";

/// `--core` value: a managed core id, or a path as in 0.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreSpec {
    Managed(String),
    Path(PathBuf),
}

impl CoreSpec {
    /// An id has no separator and no dot (`gambatte`); anything else is a path.
    fn parse(value: &OsString) -> Self {
        let text = value.to_string_lossy();
        if looks_like_core_id(&text) {
            Self::Managed(text.into_owned())
        } else {
            Self::Path(PathBuf::from(value))
        }
    }
}

fn looks_like_core_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[derive(Debug, PartialEq, Eq)]
pub struct Cli {
    pub core: CoreSpec,
    pub rom: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub system_dir: Option<PathBuf>,
    pub save_dir: Option<PathBuf>,
    pub scale: Option<u32>,
    pub audio: bool,
    pub frames: Option<u64>,
    pub dump_frame: Option<PathBuf>,
    pub screen: Option<Screen>,
    pub dump_window: Option<PathBuf>,
}

/// No `--core`/`--rom`: the library window (also a double-click, or a Finder launch on macOS).
#[derive(Debug, PartialEq, Eq)]
pub struct LibraryCli {
    pub data_dir: Option<PathBuf>,
    pub system_dir: Option<PathBuf>,
    pub save_dir: Option<PathBuf>,
    pub audio: bool,
    pub screen: Option<LibraryScreen>,
    pub dump_window: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoresAction {
    List,
    Refresh,
    /// Ids or `recommended`, in command-line order.
    Install(Vec<String>),
    Update,
    Remove(String),
    CheckBios(Option<String>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct CoresCli {
    pub data_dir: Option<PathBuf>,
    pub action: CoresAction,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(Box<Cli>),
    Library(Box<LibraryCli>),
    Cores(Box<CoresCli>),
    Help,
    Version,
}

/// `emu-rust <version>`, from the crate version.
pub fn version_line() -> String {
    format!("emu-rust {}", env!("CARGO_PKG_VERSION"))
}

enum ScreenArg {
    Game(Screen),
    Library(LibraryScreen),
}

pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Command, String> {
    let mut core = None;
    let mut rom = None;
    let mut data_dir = None;
    let mut system_dir = None;
    let mut save_dir = None;
    let mut scale = None;
    let mut audio = true;
    let mut frames = None;
    let mut dump_frame = None;
    let mut screen = None;
    let mut dump_window = None;
    let mut actions: Vec<CoresAction> = Vec::new();

    let mut it = args.into_iter().peekable();
    while let Some(arg) = it.next() {
        let name = arg.to_string_lossy().into_owned();
        if name == "--help" || name == "-h" {
            return Ok(Command::Help);
        }
        if name == "--version" || name == "-V" {
            return Ok(Command::Version);
        }
        // Finder (macOS) passes `-psn_0_12345` when launching an app bundle.
        if name.starts_with("-psn_") {
            continue;
        }
        let value = |it: &mut std::iter::Peekable<I::IntoIter>| {
            it.next().ok_or_else(|| format!("{name} attend une valeur"))
        };
        let text = |it: &mut std::iter::Peekable<I::IntoIter>| {
            value(it).map(|v| v.to_string_lossy().into_owned())
        };
        match name.as_str() {
            "--core" => core = Some(CoreSpec::parse(&value(&mut it)?)),
            "--rom" => rom = Some(PathBuf::from(value(&mut it)?)),
            "--data-dir" => data_dir = Some(PathBuf::from(value(&mut it)?)),
            "--dump-frame" => dump_frame = Some(PathBuf::from(value(&mut it)?)),
            "--dump-window" => dump_window = Some(PathBuf::from(value(&mut it)?)),
            "--system-dir" => system_dir = Some(PathBuf::from(value(&mut it)?)),
            "--save-dir" => save_dir = Some(PathBuf::from(value(&mut it)?)),
            "--no-audio" => audio = false,
            "--screen" => {
                screen = Some(match value(&mut it)?.to_string_lossy().as_ref() {
                    "menu" => ScreenArg::Game(Screen::Menu),
                    "keys" => ScreenArg::Game(Screen::Keys),
                    "options" => ScreenArg::Game(Screen::CoreOptions),
                    "library" => ScreenArg::Library(LibraryScreen::Home),
                    "library-settings" => ScreenArg::Library(LibraryScreen::Settings),
                    "cores" => ScreenArg::Library(LibraryScreen::Cores),
                    "library-launch" => ScreenArg::Library(LibraryScreen::Launch),
                    _ => {
                        return Err("--screen attend menu, keys, options, library, \
                                    library-settings, library-launch ou cores"
                            .into());
                    }
                });
            }
            "--scale" => {
                let n: u32 = number(&name, &value(&mut it)?)?;
                if !(MIN_SCALE..=MAX_SCALE).contains(&n) {
                    return Err(format!(
                        "--scale doit être entre {MIN_SCALE} et {MAX_SCALE}"
                    ));
                }
                scale = Some(n);
            }
            "--frames" => frames = Some(number(&name, &value(&mut it)?)?),
            "--list-cores" => actions.push(CoresAction::List),
            "--refresh-cores" => actions.push(CoresAction::Refresh),
            "--update-cores" => actions.push(CoresAction::Update),
            "--remove-core" => {
                actions.push(CoresAction::Remove(core_id_arg(&name, text(&mut it)?)?))
            }
            "--install-core" => {
                let id = text(&mut it)?;
                if id != "recommended" {
                    core_id_arg(&name, id.clone())?;
                }
                match actions.last_mut() {
                    Some(CoresAction::Install(ids)) => ids.push(id),
                    _ => actions.push(CoresAction::Install(vec![id])),
                }
            }
            "--check-bios" => {
                let id = match it.peek() {
                    Some(next) if !next.to_string_lossy().starts_with('-') => {
                        Some(core_id_arg(&name, text(&mut it)?)?)
                    }
                    _ => None,
                };
                actions.push(CoresAction::CheckBios(id));
            }
            _ => return Err(format!("option inconnue : {name}")),
        }
    }

    if let Some(first) = actions.first() {
        if actions.len() > 1 {
            return Err("une seule commande de gestion des cores à la fois \
                        (--install-core peut être répété)"
                .into());
        }
        if core.is_some() || rom.is_some() || screen.is_some() {
            return Err(
                "les commandes de gestion des cores ne se combinent pas avec \
                        --core, --rom ni --screen"
                    .into(),
            );
        }
        return Ok(Command::Cores(Box::new(CoresCli {
            data_dir,
            action: first.clone(),
        })));
    }

    if core.is_none() && rom.is_none() {
        if scale.is_some() || frames.is_some() || dump_frame.is_some() {
            return Err("--scale, --frames et --dump-frame demandent --core et --rom".into());
        }
        let screen = match screen {
            None => None,
            Some(ScreenArg::Library(s)) => Some(s),
            Some(ScreenArg::Game(_)) => {
                return Err("--screen menu, keys et options demandent --core et --rom".into());
            }
        };
        return Ok(Command::Library(Box::new(LibraryCli {
            data_dir,
            system_dir,
            save_dir,
            audio,
            screen,
            dump_window,
        })));
    }

    let screen = match screen {
        None => None,
        Some(ScreenArg::Game(s)) => Some(s),
        Some(ScreenArg::Library(_)) => {
            return Err(
                "--screen library, library-settings et cores s'utilisent sans --core ni --rom"
                    .into(),
            );
        }
    };
    Ok(Command::Run(Box::new(Cli {
        core: core.ok_or("--core est obligatoire")?,
        rom: rom.ok_or("--rom est obligatoire")?,
        data_dir,
        system_dir,
        save_dir,
        scale,
        audio,
        frames,
        dump_frame,
        screen,
        dump_window,
    })))
}

fn core_id_arg(name: &str, id: String) -> Result<String, String> {
    if looks_like_core_id(&id) {
        Ok(id)
    } else {
        Err(format!("{name} : identifiant de core invalide"))
    }
}

/// Data root: `--data-dir`, then `EMU_DATA_DIR` (if non-empty), then the platform data directory. Made
/// absolute (a relative root would otherwise follow the working directory).
pub fn resolve_root(
    cli: Option<PathBuf>,
    env: Option<OsString>,
    standard: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let root = cli
        .or_else(|| env.filter(|v| !v.is_empty()).map(PathBuf::from))
        .or(standard)
        .ok_or_else(|| "dossier de données introuvable : utiliser --data-dir".to_owned())?;
    std::path::absolute(&root).map_err(|e| format!("{} : chemin invalide : {e}", root.display()))
}

fn number<T: std::str::FromStr>(name: &str, v: &OsString) -> Result<T, String> {
    v.to_string_lossy()
        .parse()
        .map_err(|_| format!("{name} attend un nombre entier positif"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn defaults() {
        let Command::Run(o) = p(&["--core", "c.so", "--rom", "r.bin"]).unwrap() else {
            panic!()
        };
        assert_eq!(o.scale, None);
        assert!(o.audio);
        assert_eq!(o.frames, None);
        assert_eq!(o.data_dir, None);
        assert_eq!(o.screen, None);
    }

    #[test]
    fn all_options() {
        let Command::Run(o) = p(&[
            "--core",
            "c.so",
            "--rom",
            "r.bin",
            "--scale",
            "2",
            "--frames",
            "600",
            "--dump-frame",
            "o.png",
            "--system-dir",
            "s",
            "--save-dir",
            "v",
            "--data-dir",
            "d",
            "--screen",
            "keys",
            "--dump-window",
            "w.png",
        ])
        .unwrap() else {
            panic!()
        };
        assert_eq!(o.scale, Some(2));
        assert_eq!(o.frames, Some(600));
        assert_eq!(o.dump_frame, Some(PathBuf::from("o.png")));
        assert_eq!(o.system_dir, Some(PathBuf::from("s")));
        assert_eq!(o.save_dir, Some(PathBuf::from("v")));
        assert_eq!(o.data_dir, Some(PathBuf::from("d")));
        assert_eq!(o.screen, Some(Screen::Keys));
        assert_eq!(o.core, CoreSpec::Path(PathBuf::from("c.so")));
        assert_eq!(o.dump_window, Some(PathBuf::from("w.png")));
    }

    #[test]
    fn no_audio() {
        let Command::Run(o) = p(&["--core", "c", "--rom", "r", "--no-audio"]).unwrap() else {
            panic!()
        };
        assert!(!o.audio);
    }

    #[test]
    fn errors() {
        assert!(p(&["--rom", "r"]).unwrap_err().contains("--core"));
        assert!(p(&["--core", "c"]).unwrap_err().contains("--rom"));
        assert!(p(&["--core"]).is_err());
        assert!(p(&["--core", "c", "--rom", "r", "--scale", "0"]).is_err());
        assert!(p(&["--core", "c", "--rom", "r", "--scale", "17"]).is_err());
        assert!(p(&["--core", "c", "--rom", "r", "--scale", "4294967295"]).is_err());
        assert!(p(&["--core", "c", "--rom", "r", "--frames", "x"]).is_err());
        assert!(p(&["--core", "c", "--rom", "r", "--screen", "x"]).is_err());
        assert!(p(&["--bogus"]).unwrap_err().contains("inconnue"));
        assert_eq!(p(&["--help"]).unwrap(), Command::Help);
        assert_eq!(p(&["--version"]).unwrap(), Command::Version);
        assert_eq!(p(&["-V"]).unwrap(), Command::Version);
        assert!(matches!(p(&[]).unwrap(), Command::Library(_)));
        assert!(version_line().starts_with("emu-rust "));
    }

    #[test]
    fn root_precedence() {
        let s = |x: &str| Some(PathBuf::from(x));
        let abs = |x: &str| std::path::absolute(x).unwrap();
        let e = |x: &str| Some(OsString::from(x));
        assert_eq!(resolve_root(s("a"), e("b"), s("c")).unwrap(), abs("a"));
        assert_eq!(resolve_root(None, e("b"), s("c")).unwrap(), abs("b"));
        assert_eq!(resolve_root(None, e(""), s("c")).unwrap(), abs("c"));
        assert!(resolve_root(None, None, None).is_err());
    }

    #[test]
    fn core_id_or_path() {
        let core = |c: &str| {
            let Command::Run(o) = p(&["--core", c, "--rom", "r"]).unwrap() else {
                panic!()
            };
            o.core
        };
        assert_eq!(core("gambatte"), CoreSpec::Managed("gambatte".into()));
        assert_eq!(core("./gambatte"), CoreSpec::Path("./gambatte".into()));
        assert!(matches!(core("a/b"), CoreSpec::Path(_)));
        assert!(matches!(core("g.so"), CoreSpec::Path(_)));
    }

    #[test]
    fn library_mode() {
        let Command::Library(l) = p(&["--screen", "library-settings", "--no-audio"]).unwrap()
        else {
            panic!()
        };
        assert_eq!(l.screen, Some(LibraryScreen::Settings));
        assert!(!l.audio);
        let Command::Library(l) = p(&["--screen", "library-launch"]).unwrap() else {
            panic!()
        };
        assert_eq!(l.screen, Some(LibraryScreen::Launch));
        let Command::Library(l) = p(&["-psn_0_12345", "--data-dir", "d"]).unwrap() else {
            panic!()
        };
        assert_eq!(l.data_dir, Some(PathBuf::from("d")));
        assert_eq!(l.screen, None);
        let Command::Library(l) = p(&["--screen", "cores"]).unwrap() else {
            panic!()
        };
        assert_eq!(l.screen, Some(LibraryScreen::Cores));
        assert!(matches!(p(&["-psn_0_1"]).unwrap(), Command::Library(_)));
        assert!(p(&["--screen", "menu"]).is_err());
        assert!(p(&["--scale", "2"]).is_err());
        assert!(p(&["--core", "c", "--rom", "r", "--screen", "library"]).is_err());
    }

    fn cores(args: &[&str]) -> CoresAction {
        let Command::Cores(c) = p(args).unwrap() else {
            panic!()
        };
        c.action
    }

    #[test]
    fn cores_commands() {
        assert_eq!(cores(&["--list-cores"]), CoresAction::List);
        assert_eq!(cores(&["--refresh-cores"]), CoresAction::Refresh);
        assert_eq!(cores(&["--update-cores"]), CoresAction::Update);
        assert_eq!(
            cores(&["--remove-core", "gambatte"]),
            CoresAction::Remove("gambatte".into())
        );
        assert_eq!(
            cores(&["--install-core", "recommended"]),
            CoresAction::Install(vec!["recommended".into()])
        );
        assert_eq!(
            cores(&["--install-core", "mesen", "--install-core", "snes9x"]),
            CoresAction::Install(vec!["mesen".into(), "snes9x".into()])
        );
        assert_eq!(cores(&["--check-bios"]), CoresAction::CheckBios(None));
        assert_eq!(
            cores(&["--check-bios", "--data-dir", "d"]),
            CoresAction::CheckBios(None)
        );
        assert_eq!(
            cores(&["--check-bios", "snes9x"]),
            CoresAction::CheckBios(Some("snes9x".into()))
        );
        let Command::Cores(c) = p(&["--data-dir", "d", "--list-cores"]).unwrap() else {
            panic!()
        };
        assert_eq!(c.data_dir, Some(PathBuf::from("d")));
    }

    #[test]
    fn cores_errors() {
        assert!(p(&["--install-core"]).is_err());
        assert!(p(&["--install-core", "../x"]).is_err());
        assert!(p(&["--remove-core", "a/b"]).is_err());
        assert!(p(&["--list-cores", "--update-cores"]).is_err());
        assert!(p(&["--list-cores", "--core", "c", "--rom", "r"]).is_err());
        assert!(p(&["--list-cores", "--screen", "cores"]).is_err());
    }
}
