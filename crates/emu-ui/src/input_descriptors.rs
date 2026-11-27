//! Names of the buttons a core announces for its pad (`RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS`, ADR 0025 §11).
//! Kept in `<data>/cache/input-descriptors/<core>.toml` after a dry run that passed and at the end of a game, so
//! that the screens that remap the keys show « A — Tir » instead of « A » even before the core is running. The file
//! is cache: missing, oversized or abîmé means « no names » (the generic A, B, X, Y), never an error. Bounded in
//! entries, in size and in the length of each name; the names are text from the core, cleaned before they are shown.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use emu_runtime::{InputDescriptor, JoypadButton, MAX_INPUT_DESCRIPTORS};
use serde::{Deserialize, Serialize};

/// Largest file read.
const MAX_FILE_BYTES: u64 = 64 * 1024;
/// Characters kept from a name.
const MAX_NAME_CHARS: usize = 64;
/// `RETRO_DEVICE_JOYPAD`.
const DEVICE_JOYPAD: u32 = 1;
const DIR: &str = "input-descriptors";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    port: u32,
    device: u32,
    index: u32,
    id: u32,
    description: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileContent {
    #[serde(default)]
    descriptor: Vec<Entry>,
}

/// `<cache>/input-descriptors/<core>.toml`; `None` for a name that is not a core id (never a path).
fn file(cache_dir: &Path, core: &str) -> Option<PathBuf> {
    emu_assets::cores::CoreId::new(core)?;
    Some(cache_dir.join(DIR).join(format!("{core}.toml")))
}

fn clean(description: &str) -> String {
    crate::sanitize_core_line(description)
        .chars()
        .take(MAX_NAME_CHARS)
        .collect()
}

/// Writes the descriptors of `core`. An empty list never replaces a file (a core that announced nothing this time
/// keeps what it announced before); an identical file is not rewritten. `Ok(true)`: written.
pub(crate) fn save(
    cache_dir: &Path,
    core: &str,
    descriptors: &[InputDescriptor],
) -> io::Result<bool> {
    let Some(path) = file(cache_dir, core) else {
        return Ok(false);
    };
    let entries: Vec<Entry> = descriptors
        .iter()
        .map(|d| Entry {
            port: d.port,
            device: d.device,
            index: d.index,
            id: d.id,
            description: clean(&d.description),
        })
        .filter(|e| !e.description.is_empty())
        .take(MAX_INPUT_DESCRIPTORS)
        .collect();
    if entries.is_empty() {
        return Ok(false);
    }
    let text = render(entries, MAX_FILE_BYTES)?;
    if fs::read_to_string(&path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    emu_runtime::write_atomic(&path, text.as_bytes(), emu_runtime::FileMode::Default)?;
    Ok(true)
}

/// The file text; entries are dropped from the end until it fits `max_bytes`, so that what is written is always
/// read back (a file over the bound reads as « no names »).
fn render(mut entries: Vec<Entry>, max_bytes: u64) -> io::Result<String> {
    loop {
        let text = toml::to_string(&FileContent {
            descriptor: entries.clone(),
        })
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if text.len() as u64 <= max_bytes || entries.is_empty() {
            return Ok(text);
        }
        // Halving keeps the loop short; the first entries are the ones the screens use.
        entries.truncate(entries.len() / 2);
    }
}

/// What `save` kept for `core` (empty when there is nothing, or the file is not usable).
pub(crate) fn load(cache_dir: &Path, core: &str) -> Vec<InputDescriptor> {
    let Some(path) = file(cache_dir, core) else {
        return Vec::new();
    };
    let mut text = String::new();
    let read = fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_string(&mut text))
        .is_ok();
    if !read || text.len() as u64 > MAX_FILE_BYTES {
        return Vec::new();
    }
    let Ok(content) = toml::from_str::<FileContent>(&text) else {
        return Vec::new();
    };
    content
        .descriptor
        .into_iter()
        .map(|e| InputDescriptor {
            port: e.port,
            device: e.device,
            index: e.index,
            id: e.id,
            description: clean(&e.description),
        })
        .filter(|d| !d.description.is_empty())
        .take(MAX_INPUT_DESCRIPTORS)
        .collect()
}

/// Names of the buttons of the pad of port 0, first entry per button. Cleaned again: this is text of a core, or of
/// a file the user may have edited.
pub(crate) fn pad_names(descriptors: &[InputDescriptor]) -> Vec<(JoypadButton, String)> {
    let mut names: Vec<(JoypadButton, String)> = Vec::new();
    for d in descriptors
        .iter()
        .filter(|d| d.port == 0 && d.device == DEVICE_JOYPAD)
    {
        let Some(button) = JoypadButton::ALL.iter().find(|b| **b as u32 == d.id) else {
            continue;
        };
        let name = clean(&d.description);
        if !name.is_empty() && !names.iter().any(|(b, _)| b == button) {
            names.push((*button, name));
        }
    }
    names
}

/// The name a core gave to `button`, if it did.
pub(crate) fn name_of(names: &[(JoypadButton, String)], button: JoypadButton) -> Option<&str> {
    names
        .iter()
        .find(|(b, _)| *b == button)
        .map(|(_, n)| n.as_str())
}

/// Names announced by the running core, else the ones kept from an earlier run or dry run.
pub(crate) fn names_for(
    running: &[InputDescriptor],
    cache_dir: &Path,
    core: &str,
) -> Vec<(JoypadButton, String)> {
    let live = pad_names(running);
    if live.is_empty() {
        pad_names(&load(cache_dir, core))
    } else {
        live
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(port: u32, device: u32, id: u32, text: &str) -> InputDescriptor {
        InputDescriptor {
            port,
            device,
            index: 0,
            id,
            description: text.to_owned(),
        }
    }

    #[test]
    fn what_a_core_announced_comes_back_from_the_cache_and_names_the_pad_of_port_zero() {
        let dir = tempfile::tempdir().unwrap();
        let announced = [
            d(0, 1, 8, "Tir"),
            d(0, 1, 0, "Saut"),
            d(1, 1, 9, "Tir (joueur 2)"),
            d(0, 3, 0, "Souris : bouton gauche"),
            d(0, 1, 8, "Doublon"),
        ];
        assert!(save(dir.path(), "vecx", &announced).unwrap());
        let back = load(dir.path(), "vecx");
        assert_eq!(back.len(), 5);
        let names = pad_names(&back);
        assert_eq!(name_of(&names, JoypadButton::A), Some("Tir"), "first entry");
        assert_eq!(name_of(&names, JoypadButton::B), Some("Saut"));
        assert_eq!(
            name_of(&names, JoypadButton::X),
            None,
            "no name: the generic one stays"
        );
        assert_eq!(
            names.len(),
            2,
            "port 1 and the mouse are not the pad of port 0"
        );
    }

    #[test]
    fn the_written_text_never_exceeds_the_bound_the_reader_accepts() {
        let entries: Vec<Entry> = (0..200)
            .map(|i| Entry {
                port: 0,
                device: 1,
                index: 0,
                id: i,
                description: "x".repeat(60),
            })
            .collect();
        assert!(render(entries.clone(), u64::MAX).unwrap().len() > 4_000);
        let text = render(entries, 4_000).unwrap();
        assert!(text.len() <= 4_000, "{}", text.len());
        let back: FileContent = toml::from_str(&text).unwrap();
        assert!(!back.descriptor.is_empty());
    }

    #[test]
    fn an_empty_announcement_never_erases_the_file_and_an_identical_one_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save(dir.path(), "vecx", &[d(0, 1, 8, "Tir")]).unwrap());
        assert!(
            !save(dir.path(), "vecx", &[d(0, 1, 8, "Tir")]).unwrap(),
            "unchanged"
        );
        assert!(!save(dir.path(), "vecx", &[]).unwrap());
        assert!(
            !save(dir.path(), "vecx", &[d(0, 1, 8, " \u{202e} ")]).unwrap(),
            "nothing left once cleaned"
        );
        assert_eq!(pad_names(&load(dir.path(), "vecx")).len(), 1);
    }

    #[test]
    fn names_are_cleaned_bounded_and_a_core_name_is_never_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let long = "x".repeat(500);
        assert!(
            save(
                dir.path(),
                "vecx",
                &[d(0, 1, 8, &format!("A\u{202e}\nB{long}"))]
            )
            .unwrap()
        );
        let names = pad_names(&load(dir.path(), "vecx"));
        let name = &names[0].1;
        assert!(name.chars().count() <= MAX_NAME_CHARS, "{name}");
        assert!(
            !name.contains('\u{202e}') && !name.contains('\n'),
            "{name:?}"
        );
        // A name with a separator is not a core id: nothing is read or written outside the folder.
        assert!(!save(dir.path(), "../evil", &[d(0, 1, 8, "x")]).unwrap());
        assert!(load(dir.path(), "../evil").is_empty());
        assert!(!dir.path().join("evil.toml").exists());
    }

    #[test]
    fn a_broken_or_oversized_file_means_no_names() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(DIR);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("vecx.toml"), "descriptor = [oops").unwrap();
        assert!(load(dir.path(), "vecx").is_empty());
        fs::write(folder.join("big.toml"), "#".repeat(70_000)).unwrap();
        assert!(load(dir.path(), "big").is_empty());
    }

    #[test]
    fn the_running_core_wins_over_the_cache_and_the_cache_serves_before_it_runs() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "vecx", &[d(0, 1, 8, "Ancien")]).unwrap();
        let live = [d(0, 1, 8, "Actuel")];
        assert_eq!(
            name_of(&names_for(&live, dir.path(), "vecx"), JoypadButton::A),
            Some("Actuel")
        );
        assert_eq!(
            name_of(&names_for(&[], dir.path(), "vecx"), JoypadButton::A),
            Some("Ancien")
        );
        assert!(names_for(&[], dir.path(), "other").is_empty());
    }
}
