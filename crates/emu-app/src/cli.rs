//! Hand-written argument parser (no `clap` for a few dozen options). Messages are user-facing French.
//! Pure: data directories and `config.toml` are resolved afterwards by `main`. Core ids are only checked
//! syntactically here; `emu_assets::cores::CoreId` validates them when the command runs.

use std::ffi::OsString;
use std::path::PathBuf;

use emu_ui::{LibraryScreen, MAX_SCALE, MIN_SCALE, Screen};

pub const USAGE: &str = "Usage : emu-rust                         ouvre la bibliothèque de jeux\n\
                         \x20      emu-rust --core <ID|chemin> [--rom <fichier> [--rom <disquette 2> ...] | --no-game]\n\
                         \x20                [--data-dir DIR] [--system-dir DIR] [--save-dir DIR] [--scale N]\n\
                         \x20                [--no-audio] [--no-gamepad] [--frames N] [--dump-frame sortie.png]\n\
                         \x20                [--launch <commande|programme>] [--no-auto-fast-load]\n\
                         \x20                [--pad-device digital|dualshock]\n\
                         \x20                [--screen menu|keys|options|disks|computer|vkbd] [--dump-window sortie.png]\n\
                         \x20      emu-rust [--data-dir DIR] [--no-audio] [--no-gamepad]\n\
                         \x20                [--screen library|library-settings|cores|library-log|library-tidy|library-launch]\n\
                         \x20                [--dump-window sortie.png]\n\
                         \x20      emu-rust [--data-dir DIR] --list-cores | --refresh-cores | --update-cores\n\
                         \x20      emu-rust [--data-dir DIR] --install-core <ID|recommended> [--install-core ...]\n\
                         \x20      emu-rust [--data-dir DIR] --remove-core <ID> | --check-bios [ID] | --list-bios\n\
                         \x20      emu-rust [--data-dir DIR] --install-bios emutos[:VARIANTE] | generaluser-gs\n\
                         \x20                [--replace-user-bios]\n\
                         \x20      emu-rust [--data-dir DIR] --clear-dos-cache\n\
                         \x20      emu-rust [--data-dir DIR] --import-bios <fichier> [--bios-name <nom> --unverified]\n\
                         \x20                [--replace-user-bios]\n\
                         \x20      emu-rust --version | --help\n\
                         --core accepte l'identifiant d'un core installé (ex. gambatte, puae, puae2021) ou le\n\
                         chemin d'un fichier.\n\
                         --import-bios copie un BIOS que vous possédez (Kickstart, TOS…) sous le nom attendu, après\n\
                         vérification de son empreinte ; un fichier inconnu demande --bios-name (nom listé par\n\
                         --list-bios) et --unverified.\n\
                         --rom répété : disquettes d'un même jeu, dans l'ordre ; sans --rom : démarrage sans\n\
                         contenu (bureau d'un ordinateur, ex. --core hatari).\n\
                         PC-DOS (--core dosbox_pure) : --rom accepte un zip, une image, un dossier de jeu (copié\n\
                         dans le cache, jamais modifié) ou JEU/PROGRAMME.EXE ; les fichiers .conf sont refusés.\n\
                         --clear-dos-cache vide ces copies (sauvegardes conservées).\n\
                         Amstrad CPC / Commodore 64 : --launch donne la commande de démarrage (ex. RUN\"DISC) ou\n\
                         le programme de la disquette à lancer (ex. INTRO) ; --no-auto-fast-load désactive le\n\
                         chargement accéléré demandé par le core.\n\
                         PlayStation : --pad-device choisit le pad (numérique ou DualShock analogique) pour cette\n\
                         session, sans le mémoriser.\n\
                         --no-gamepad : manettes ignorées (clavier seul). Manette PS4 : PS + Options = menu,\n\
                         PS + R1 / L1 = sauver / charger l'état, PS + R2 maintenu = avance rapide ;\n\
                         ordinateurs : PS + R3 = clavier virtuel, PS + L3 = disquette suivante.\n\
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
        // 0.1 compatibility: an existing file (even extension-less) wins over a managed id.
        if looks_like_core_id(&text) && !std::path::Path::new(value).is_file() {
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
    /// Disks of one game in order; empty = no content (`--no-game`, or `--core` alone).
    pub roms: Vec<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub system_dir: Option<PathBuf>,
    pub save_dir: Option<PathBuf>,
    pub scale: Option<u32>,
    pub audio: bool,
    /// `false` with `--no-gamepad`.
    pub gamepad: bool,
    pub frames: Option<u64>,
    pub dump_frame: Option<PathBuf>,
    pub screen: Option<Screen>,
    pub dump_window: Option<PathBuf>,
    /// `--launch` (design v5.0 §4): checked against the core when it is loaded.
    pub launch: Option<String>,
    /// `false` with `--no-auto-fast-load`.
    pub auto_fast_load: bool,
    /// `--pad-device digital|dualshock` (design v7.0 §3.5): pad mode of this run, over the game's and the
    /// config's; checked against the core's `pad_devices` when it is loaded.
    pub pad_device: Option<String>,
}

/// No `--core`/`--rom`: the library window (also a double-click, or a Finder launch on macOS).
#[derive(Debug, PartialEq, Eq)]
pub struct LibraryCli {
    pub data_dir: Option<PathBuf>,
    pub system_dir: Option<PathBuf>,
    pub save_dir: Option<PathBuf>,
    pub audio: bool,
    pub gamepad: bool,
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
    ListBios,
    /// `--install-bios id[:variant]`; `replace`: `--replace-user-bios` (backup then replace a user file).
    InstallBios {
        id: String,
        variant: Option<String>,
        replace: bool,
    },
    /// `--clear-dos-cache`: snapshots of the DOS folder games (design v4.0 §4).
    ClearDosCache,
    /// `--import-bios <file>` (design v3.0 §4): `name` = `--bios-name`, `unverified` = `--unverified` (both
    /// needed for an unknown file), `replace` = `--replace-user-bios`.
    ImportBios {
        file: PathBuf,
        name: Option<String>,
        unverified: bool,
        replace: bool,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct CoresCli {
    pub data_dir: Option<PathBuf>,
    /// Used by `--check-bios` (and by the manager for BIOS paths).
    pub system_dir: Option<PathBuf>,
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
    let mut roms: Vec<PathBuf> = Vec::new();
    let mut no_game = false;
    let mut replace_user_bios = false;
    let mut bios_name: Option<String> = None;
    let mut unverified = false;
    let mut data_dir = None;
    let mut system_dir = None;
    let mut save_dir = None;
    let mut scale = None;
    let mut audio = true;
    let mut gamepad = true;
    let mut frames = None;
    let mut dump_frame = None;
    let mut screen = None;
    let mut dump_window = None;
    let mut launch: Option<String> = None;
    let mut auto_fast_load = true;
    let mut pad_device: Option<String> = None;
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
            "--rom" => roms.push(PathBuf::from(value(&mut it)?)),
            "--no-game" => no_game = true,
            "--list-bios" => actions.push(CoresAction::ListBios),
            "--clear-dos-cache" => actions.push(CoresAction::ClearDosCache),
            "--replace-user-bios" => replace_user_bios = true,
            "--import-bios" => actions.push(CoresAction::ImportBios {
                file: PathBuf::from(value(&mut it)?),
                name: None,
                unverified: false,
                replace: false,
            }),
            "--bios-name" => {
                let n = text(&mut it)?;
                // Names come from the catalogue; checked again when the command runs.
                if n.is_empty()
                    || n.contains("..")
                    || n.starts_with(['/', '\\'])
                    || n.contains('\\')
                {
                    return Err("--bios-name : nom de BIOS invalide".into());
                }
                bios_name = Some(n);
            }
            "--unverified" => unverified = true,
            "--install-bios" => {
                let spec = text(&mut it)?;
                let (id, variant) = match spec.split_once(':') {
                    Some((id, v)) => (id.to_owned(), Some(v.to_owned())),
                    None => (spec, None),
                };
                let ok = |t: &str| looks_like_core_id(t);
                if !ok(&id) || variant.as_deref().is_some_and(|v| !ok(v)) {
                    return Err(
                        "--install-bios attend un identifiant de --list-bios (ex. emutos:192fr, generaluser-gs)"
                            .into(),
                    );
                }
                actions.push(CoresAction::InstallBios {
                    id,
                    variant,
                    replace: false,
                });
            }
            "--data-dir" => data_dir = Some(PathBuf::from(value(&mut it)?)),
            "--dump-frame" => dump_frame = Some(PathBuf::from(value(&mut it)?)),
            "--dump-window" => dump_window = Some(PathBuf::from(value(&mut it)?)),
            "--system-dir" => system_dir = Some(PathBuf::from(value(&mut it)?)),
            "--save-dir" => save_dir = Some(PathBuf::from(value(&mut it)?)),
            "--no-audio" => audio = false,
            "--no-gamepad" => gamepad = false,
            "--no-auto-fast-load" => auto_fast_load = false,
            "--pad-device" => {
                let t = text(&mut it)?;
                if t != "digital" && t != "dualshock" {
                    return Err("--pad-device attend digital ou dualshock".into());
                }
                pad_device = Some(t);
            }
            "--launch" => {
                let t = text(&mut it)?;
                if t.trim().is_empty() {
                    return Err("--launch attend une commande ou un nom de programme".into());
                }
                launch = Some(t);
            }
            "--screen" => {
                screen = Some(match value(&mut it)?.to_string_lossy().as_ref() {
                    "menu" => ScreenArg::Game(Screen::Menu),
                    "keys" => ScreenArg::Game(Screen::Keys),
                    "options" => ScreenArg::Game(Screen::CoreOptions),
                    "disks" => ScreenArg::Game(Screen::Disks),
                    "computer" => ScreenArg::Game(Screen::Computer),
                    "vkbd" => ScreenArg::Game(Screen::VirtualKeyboard),
                    "library" => ScreenArg::Library(LibraryScreen::Home),
                    "library-settings" => ScreenArg::Library(LibraryScreen::Settings),
                    "cores" => ScreenArg::Library(LibraryScreen::Cores),
                    "library-log" => ScreenArg::Library(LibraryScreen::Log),
                    "library-tidy" => ScreenArg::Library(LibraryScreen::Tidy),
                    "library-launch" => ScreenArg::Library(LibraryScreen::Launch),
                    _ => {
                        return Err(
                            "--screen attend menu, keys, options, disks, computer, vkbd, library, \
                                    library-settings, library-log, library-tidy, library-launch ou cores"
                                .into(),
                        );
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

    if replace_user_bios {
        match actions.last_mut() {
            Some(
                CoresAction::InstallBios { replace, .. } | CoresAction::ImportBios { replace, .. },
            ) => *replace = true,
            _ => {
                return Err(
                    "--replace-user-bios s'utilise avec --install-bios ou --import-bios".into(),
                );
            }
        }
    }
    if bios_name.is_some() || unverified {
        match actions.last_mut() {
            Some(CoresAction::ImportBios {
                name,
                unverified: u,
                ..
            }) => {
                *name = bios_name.take();
                *u = unverified;
            }
            _ => return Err("--bios-name et --unverified s'utilisent avec --import-bios".into()),
        }
    }
    if no_game && !roms.is_empty() {
        return Err("--no-game et --rom s'excluent".into());
    }
    if no_game && core.is_none() {
        return Err("--no-game demande --core".into());
    }
    let rom = roms.first().cloned();
    if let Some(first) = actions.first() {
        if actions.len() > 1 {
            return Err("une seule commande de gestion des cores à la fois \
                        (--install-core peut être répété)"
                .into());
        }
        if core.is_some() || rom.is_some() || no_game || screen.is_some() {
            return Err(
                "les commandes de gestion des cores ne se combinent pas avec \
                        --core, --rom ni --screen"
                    .into(),
            );
        }
        let ignored: Vec<&str> = [
            (save_dir.is_some(), "--save-dir"),
            (!audio, "--no-audio"),
            (!gamepad, "--no-gamepad"),
            (scale.is_some(), "--scale"),
            (frames.is_some(), "--frames"),
            (dump_frame.is_some(), "--dump-frame"),
            (dump_window.is_some(), "--dump-window"),
            (launch.is_some(), "--launch"),
            (!auto_fast_load, "--no-auto-fast-load"),
            (pad_device.is_some(), "--pad-device"),
        ]
        .into_iter()
        .filter_map(|(set, name)| set.then_some(name))
        .collect();
        if !ignored.is_empty() {
            return Err(format!(
                "sans effet avec une commande de gestion des cores : {}",
                ignored.join(", ")
            ));
        }
        return Ok(Command::Cores(Box::new(CoresCli {
            data_dir,
            system_dir,
            action: first.clone(),
        })));
    }

    if core.is_none() && rom.is_none() {
        if scale.is_some() || frames.is_some() || dump_frame.is_some() {
            return Err("--scale, --frames et --dump-frame demandent --core et --rom".into());
        }
        if launch.is_some() || !auto_fast_load || pad_device.is_some() {
            return Err("--launch, --no-auto-fast-load et --pad-device demandent --core".into());
        }
        let screen = match screen {
            None => None,
            Some(ScreenArg::Library(s)) => Some(s),
            Some(ScreenArg::Game(_)) => {
                return Err(
                    "--screen menu, keys, options, disks, computer et vkbd demandent --core".into(),
                );
            }
        };
        return Ok(Command::Library(Box::new(LibraryCli {
            data_dir,
            system_dir,
            save_dir,
            audio,
            gamepad,
            screen,
            dump_window,
        })));
    }

    let screen = match screen {
        None => None,
        Some(ScreenArg::Game(s)) => Some(s),
        Some(ScreenArg::Library(_)) => {
            return Err(
                "--screen library, library-settings, cores et library-launch \
                        s'utilisent sans --core ni --rom"
                    .into(),
            );
        }
    };
    if launch.is_some() && rom.is_none() {
        return Err("--launch demande --rom".into());
    }
    Ok(Command::Run(Box::new(Cli {
        core: core.ok_or("--core est obligatoire")?,
        roms,
        data_dir,
        system_dir,
        save_dir,
        scale,
        audio,
        gamepad,
        frames,
        dump_frame,
        screen,
        dump_window,
        launch,
        auto_fast_load,
        pad_device,
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
    fn several_disks_or_none() {
        let Command::Run(o) = p(&["--core", "hatari", "--rom", "a.st", "--rom", "b.st"]).unwrap()
        else {
            panic!()
        };
        assert_eq!(o.roms, vec![PathBuf::from("a.st"), PathBuf::from("b.st")]);
        for args in [
            &["--core", "hatari"][..],
            &["--core", "hatari", "--no-game"],
        ] {
            let Command::Run(o) = p(args).unwrap() else {
                panic!()
            };
            assert!(o.roms.is_empty());
        }
        let Command::Run(o) = p(&["--core", "hatari", "--screen", "vkbd"]).unwrap() else {
            panic!()
        };
        assert_eq!(o.screen, Some(Screen::VirtualKeyboard));
    }

    #[test]
    fn bios_commands() {
        assert_eq!(cores(&["--list-bios"]), CoresAction::ListBios);
        assert_eq!(cores(&["--clear-dos-cache"]), CoresAction::ClearDosCache);
        assert_eq!(
            cores(&["--install-bios", "generaluser-gs"]),
            CoresAction::InstallBios {
                id: "generaluser-gs".into(),
                variant: None,
                replace: false,
            }
        );
        assert_eq!(
            cores(&["--install-bios", "emutos"]),
            CoresAction::InstallBios {
                id: "emutos".into(),
                variant: None,
                replace: false
            }
        );
        assert_eq!(
            cores(&["--install-bios", "emutos:192fr", "--replace-user-bios"]),
            CoresAction::InstallBios {
                id: "emutos".into(),
                variant: Some("192fr".into()),
                replace: true
            }
        );
        assert!(p(&["--replace-user-bios"]).is_err());
        assert_eq!(
            cores(&["--import-bios", "/x/kick.rom"]),
            CoresAction::ImportBios {
                file: PathBuf::from("/x/kick.rom"),
                name: None,
                unverified: false,
                replace: false,
            }
        );
        assert_eq!(
            cores(&[
                "--import-bios",
                "k.rom",
                "--bios-name",
                "kick34005.A500",
                "--unverified",
                "--replace-user-bios"
            ]),
            CoresAction::ImportBios {
                file: PathBuf::from("k.rom"),
                name: Some("kick34005.A500".into()),
                unverified: true,
                replace: true,
            }
        );
        assert!(p(&["--bios-name", "x"]).is_err());
        assert!(p(&["--unverified"]).is_err());
        assert!(p(&["--import-bios", "k", "--bios-name", "../x"]).is_err());
        assert!(p(&["--import-bios", "k", "--bios-name", "/abs"]).is_err());
        assert!(p(&["--import-bios", "k", "--core", "puae"]).is_err());
        assert!(p(&["--install-bios", "../x"]).is_err());
        assert!(p(&["--install-bios", "emutos:a/b"]).is_err());
        assert!(p(&["--install-bios", "emutos", "--rom", "r"]).is_err());
    }

    #[test]
    fn launch_and_fast_load() {
        let Command::Run(o) = p(&[
            "--core",
            "cap32",
            "--rom",
            "s.dsk",
            "--launch",
            "RUN\"SORCERY",
            "--no-auto-fast-load",
        ])
        .unwrap() else {
            panic!()
        };
        assert_eq!(o.launch.as_deref(), Some("RUN\"SORCERY"));
        assert!(!o.auto_fast_load);
        let Command::Run(o) = p(&["--core", "c", "--rom", "r"]).unwrap() else {
            panic!()
        };
        assert!(o.launch.is_none() && o.auto_fast_load);
        assert!(p(&["--core", "cap32", "--launch", "RUN"]).is_err());
        assert!(p(&["--core", "cap32", "--rom", "r", "--launch", " "]).is_err());
        assert!(p(&["--launch", "RUN"]).is_err());
        assert!(p(&["--check-bios", "--no-auto-fast-load"]).is_err());
        assert!(USAGE.contains("--launch") && USAGE.contains("--no-auto-fast-load"));
    }

    #[test]
    fn no_audio() {
        let Command::Run(o) = p(&["--core", "c", "--rom", "r", "--no-audio"]).unwrap() else {
            panic!()
        };
        assert!(!o.audio);
        assert!(o.gamepad);
    }

    #[test]
    fn no_gamepad() {
        let Command::Run(o) = p(&["--core", "c", "--rom", "r", "--no-gamepad"]).unwrap() else {
            panic!()
        };
        assert!(!o.gamepad && o.audio);
        let Command::Library(l) = p(&["--no-gamepad"]).unwrap() else {
            panic!()
        };
        assert!(!l.gamepad);
        assert!(USAGE.contains("--no-gamepad"));
    }

    #[test]
    fn errors() {
        assert!(p(&["--rom", "r"]).unwrap_err().contains("--core"));
        assert!(p(&["--no-game"]).unwrap_err().contains("--core"));
        assert!(p(&["--core", "c", "--rom", "r", "--no-game"]).is_err());
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
        // An existing extension-less file is a path (0.1 behaviour).
        let dir = std::env::temp_dir().join(format!("emu-cli-core-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("moncore");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            CoreSpec::parse(&file.clone().into_os_string()),
            CoreSpec::Path(file)
        );
        std::fs::remove_dir_all(&dir).unwrap();
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
        for extra in [
            &["--save-dir", "s"][..],
            &["--no-audio"],
            &["--no-gamepad"],
            &["--scale", "2"],
            &["--dump-window", "w.png"],
            &["--frames", "3"],
            &["--dump-frame", "f.png"],
        ] {
            let mut args = vec!["--check-bios"];
            args.extend_from_slice(extra);
            let e = p(&args).unwrap_err();
            assert!(e.contains(extra[0]), "{e}");
        }
    }

    #[test]
    fn cores_command_keeps_system_dir() {
        let Command::Cores(c) = p(&["--system-dir", "bios", "--check-bios"]).unwrap() else {
            panic!()
        };
        assert_eq!(c.system_dir, Some(PathBuf::from("bios")));
        assert_eq!(c.action, CoresAction::CheckBios(None));
    }
}
