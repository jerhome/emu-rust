//! Hand-written argument parser (no `clap` for five options). Messages are user-facing French.

use std::ffi::OsString;
use std::path::PathBuf;

use emu_ui::LaunchOptions;

pub const USAGE: &str = "Usage : emu-rust --core <core.so> --rom <fichier> [--scale N] [--frames N]\n\
                         \x20                [--dump-frame sortie.png] [--system-dir DIR] [--save-dir DIR] [--no-audio]";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(LaunchOptions),
    Help,
}

pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Command, String> {
    let mut core = None;
    let mut rom = None;
    let mut scale = 3u32;
    let mut frames = None;
    let mut dump = None;
    let mut system_dir = PathBuf::from("./data/system");
    let mut save_dir = PathBuf::from("./data/saves");
    let mut audio = true;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let name = arg.to_string_lossy().into_owned();
        if name == "--help" || name == "-h" {
            return Ok(Command::Help);
        }
        let value =
            |it: &mut I::IntoIter| it.next().ok_or_else(|| format!("{name} attend une valeur"));
        match name.as_str() {
            "--core" => core = Some(PathBuf::from(value(&mut it)?)),
            "--rom" => rom = Some(PathBuf::from(value(&mut it)?)),
            "--dump-frame" => dump = Some(PathBuf::from(value(&mut it)?)),
            "--system-dir" => system_dir = PathBuf::from(value(&mut it)?),
            "--save-dir" => save_dir = PathBuf::from(value(&mut it)?),
            "--no-audio" => audio = false,
            "--scale" => {
                scale = number(&name, &value(&mut it)?)?;
                if scale == 0 {
                    return Err("--scale doit être ≥ 1".into());
                }
            }
            "--frames" => frames = Some(number(&name, &value(&mut it)?)?),
            _ => return Err(format!("option inconnue : {name}")),
        }
    }
    Ok(Command::Run(LaunchOptions {
        core: core.ok_or("--core est obligatoire")?,
        rom: rom.ok_or("--rom est obligatoire")?,
        system_dir,
        save_dir,
        scale,
        exit_after_frames: frames,
        dump_last_frame: dump,
        audio,
    }))
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
        assert_eq!(o.scale, 3);
        assert!(o.audio);
        assert_eq!(o.exit_after_frames, None);
        assert_eq!(o.system_dir, PathBuf::from("./data/system"));
        assert_eq!(o.save_dir, PathBuf::from("./data/saves"));
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
        ])
        .unwrap() else {
            panic!()
        };
        assert_eq!(o.scale, 2);
        assert_eq!(o.exit_after_frames, Some(600));
        assert_eq!(o.dump_last_frame, Some(PathBuf::from("o.png")));
        assert_eq!(o.system_dir, PathBuf::from("s"));
        assert_eq!(o.save_dir, PathBuf::from("v"));
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
        assert!(p(&["--core", "c", "--rom", "r", "--frames", "x"]).is_err());
        assert!(p(&["--bogus"]).unwrap_err().contains("inconnue"));
        assert_eq!(p(&["--help"]).unwrap(), Command::Help);
    }
}
