//! Memory of the dry runs that passed (`<data>/cache/dry-runs.toml`, ADR 0025 §5): a game is tried once per
//! (system, core, SHA-256 of the core file, game path); an updated core tries it again. At most
//! [`MAX_ENTRIES`] entries, the oldest dropped first. A missing, oversized or unreadable file is an empty memory:
//! the cost is one more dry run, never a failure.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Entries kept.
pub(crate) const MAX_ENTRIES: usize = 2_000;
/// Largest file read (2 000 entries of a few hundred bytes).
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const FILE_NAME: &str = "dry-runs.toml";

/// What identifies a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DryRunKey {
    /// Library system id.
    pub system: String,
    pub core: String,
    /// Lower-case hex SHA-256 of the core file.
    pub core_sha256: String,
    pub game: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    #[serde(flatten)]
    key: DryRunKey,
    unix: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileContent {
    #[serde(default)]
    entry: Vec<Entry>,
}

#[derive(Debug)]
pub(crate) struct DryRunLog {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl DryRunLog {
    /// Reads `<cache>/dry-runs.toml`.
    pub(crate) fn load(cache: &Path) -> Self {
        let path = cache.join(FILE_NAME);
        let entries = read_bounded(&path)
            .and_then(|text| toml::from_str::<FileContent>(&text).ok())
            .map(|file| file.entry)
            .unwrap_or_default();
        Self { path, entries }
    }

    pub(crate) fn passed(&self, key: &DryRunKey) -> bool {
        self.entries.iter().any(|e| &e.key == key)
    }

    /// Whether a pass of this game with this core is remembered, whatever the core file (no hashing needed).
    pub(crate) fn knows(&self, key: &DryRunKey) -> bool {
        self.entries
            .iter()
            .any(|e| e.key.system == key.system && e.key.core == key.core && e.key.game == key.game)
    }

    /// Remembers a pass (the time is refreshed when it is already there) and writes the file.
    pub(crate) fn record(&mut self, key: DryRunKey, now_unix: u64) -> io::Result<()> {
        self.entries.retain(|e| e.key != key);
        self.entries.push(Entry {
            key,
            unix: now_unix,
        });
        if self.entries.len() > MAX_ENTRIES {
            self.entries.sort_by_key(|e| e.unix);
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..excess);
        }
        let text = toml::to_string(&FileContent {
            entry: self.entries.clone(),
        })
        .map_err(io::Error::other)?;
        write_atomic(&self.path, &text)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

fn read_bounded(path: &Path) -> Option<String> {
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_FILE_BYTES).then_some(text)
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // The process id in the name: two instances never write the same temporary file.
    let tmp = path.with_extension(format!("toml.tmp-{}", std::process::id()));
    let written = fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(text.as_bytes())?;
        f.flush()
    });
    match written.and_then(|()| fs::rename(&tmp, path)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Lower-case hex SHA-256 of a file (a core: a few megabytes); `None` when it cannot be read.
pub(crate) fn core_sha256(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(game: &str, sha: &str) -> DryRunKey {
        DryRunKey {
            system: "acme".to_owned(),
            core: "acme_core".to_owned(),
            core_sha256: sha.to_owned(),
            game: game.to_owned(),
        }
    }

    #[test]
    fn a_recorded_pass_is_found_again_after_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = DryRunLog::load(dir.path());
        assert!(!log.passed(&key("a", "1")));
        log.record(key("a", "1"), 10).unwrap();
        let again = DryRunLog::load(dir.path());
        assert!(again.passed(&key("a", "1")));
    }

    #[test]
    fn an_updated_core_or_another_game_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = DryRunLog::load(dir.path());
        log.record(key("a", "1"), 10).unwrap();
        assert!(!log.passed(&key("a", "2")), "other core file");
        assert!(!log.passed(&key("b", "1")), "other game");
    }

    #[test]
    fn the_oldest_entries_are_dropped_beyond_the_bound() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = DryRunLog::load(dir.path());
        for i in 0..=MAX_ENTRIES {
            log.record(key(&format!("g{i}"), "1"), i as u64).unwrap();
        }
        assert_eq!(log.len(), MAX_ENTRIES);
        assert!(!log.passed(&key("g0", "1")), "the oldest one went");
        assert!(log.passed(&key(&format!("g{MAX_ENTRIES}"), "1")));
    }

    #[test]
    fn a_damaged_or_oversized_file_is_an_empty_memory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE_NAME), "entry = 12 [[[").unwrap();
        assert_eq!(DryRunLog::load(dir.path()).len(), 0);
        let big = vec![b'#'; usize::try_from(MAX_FILE_BYTES).unwrap() + 10];
        fs::write(dir.path().join(FILE_NAME), big).unwrap();
        assert_eq!(DryRunLog::load(dir.path()).len(), 0);
    }

    #[test]
    fn the_core_hash_is_the_sha256_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("core.bin");
        fs::write(&file, b"abc").unwrap();
        assert_eq!(
            core_sha256(&file).as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(core_sha256(&dir.path().join("absent")), None);
    }
}
