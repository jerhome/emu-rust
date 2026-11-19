//! Hand-written argument parser (no `clap` for a dozen options). Messages are user-facing French.
//! Pure: data directories and `config.toml` are resolved afterwards by `main`.

use std::ffi::OsString;
use std::path::PathBuf;

use emu_ui::{MAX_SCALE, MIN_SCALE, Screen};

pub const USAGE: &str = "Usage : emu-rust --core <core> --rom <fichier> [--data-dir DIR] [--system-dir DIR]\n\
                         \x20                [--save-dir DIR] [--scale N] [--no-audio] [--frames N]\n\
                         \x20                [--dump-frame sortie.png] [--screen menu|keys|options]\n\
                         \x20                [--dump-window sortie.png]\n\
                         \x20      emu-rust --version | --help\n\
                         Racine de données : --data-dir, sinon EMU_DATA_DIR, sinon le dossier standard.";

#[derive(Debug, PartialEq, Eq)]
pub struct Cli {
    pub core: PathBuf,
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

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(Box<Cli>),
    Help,
    Version,
    /// No argument at all (e.g. a double-click on the executable): nothing to launch.
    NoArgs,
}

/// `emu-rust <version>`, from the crate version.
pub fn version_line() -> String {
    format!("emu-rust {}", env!("CARGO_PKG_VERSION"))
}

pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Command, String> {
    let mut core = None;
    let mut rom = None;
    let mut cli = Cli {
        core: PathBuf::new(),
        rom: PathBuf::new(),
        data_dir: None,
        system_dir: None,
        save_dir: None,
        scale: None,
        audio: true,
        frames: None,
        dump_frame: None,
        screen: None,
        dump_window: None,
    };

    let mut it = args.into_iter().peekable();
    if it.peek().is_none() {
        return Ok(Command::NoArgs);
    }
    while let Some(arg) = it.next() {
        let name = arg.to_string_lossy().into_owned();
        if name == "--help" || name == "-h" {
            return Ok(Command::Help);
        }
        if name == "--version" || name == "-V" {
            return Ok(Command::Version);
        }
        let value = |it: &mut std::iter::Peekable<I::IntoIter>| {
            it.next().ok_or_else(|| format!("{name} attend une valeur"))
        };
        match name.as_str() {
            "--core" => core = Some(PathBuf::from(value(&mut it)?)),
            "--rom" => rom = Some(PathBuf::from(value(&mut it)?)),
            "--data-dir" => cli.data_dir = Some(PathBuf::from(value(&mut it)?)),
            "--dump-frame" => cli.dump_frame = Some(PathBuf::from(value(&mut it)?)),
            "--dump-window" => cli.dump_window = Some(PathBuf::from(value(&mut it)?)),
            "--system-dir" => cli.system_dir = Some(PathBuf::from(value(&mut it)?)),
            "--save-dir" => cli.save_dir = Some(PathBuf::from(value(&mut it)?)),
            "--no-audio" => cli.audio = false,
            "--screen" => {
                cli.screen = Some(match value(&mut it)?.to_string_lossy().as_ref() {
                    "menu" => Screen::Menu,
                    "keys" => Screen::Keys,
                    "options" => Screen::CoreOptions,
                    _ => return Err("--screen attend menu, keys ou options".into()),
                });
            }
            "--scale" => {
                let scale: u32 = number(&name, &value(&mut it)?)?;
                if !(MIN_SCALE..=MAX_SCALE).contains(&scale) {
                    return Err(format!(
                        "--scale doit être entre {MIN_SCALE} et {MAX_SCALE}"
                    ));
                }
                cli.scale = Some(scale);
            }
            "--frames" => cli.frames = Some(number(&name, &value(&mut it)?)?),
            _ => return Err(format!("option inconnue : {name}")),
        }
    }
    cli.core = core.ok_or("--core est obligatoire")?;
    cli.rom = rom.ok_or("--rom est obligatoire")?;
    Ok(Command::Run(Box::new(cli)))
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
        assert_eq!(p(&[]).unwrap(), Command::NoArgs);
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
}
