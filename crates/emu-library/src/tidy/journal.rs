//! Tidy journal, format version 1 (ADR 0023 §4.5, design v7.1 §3.4). Written before anything moves; read by the
//! recovery, the undo and « Annuler le dernier rangement ». This module only encodes, parses, lists and appends:
//! what the lines mean for the execution, the undo and the recovery is FB1b's.
//!
//! File: `<journal_dir>/journal-<unix>.txt` (`journal-<unix>-<k>.txt` when that name is taken), UTF-8 text, one
//! record per line ending with `\n`, never rewritten in place, `sync_all` after every appended line:
//!
//! ```text
//! emu-rust tidy 1                 header (exact)
//! R <root>                        canonical roms root, absolute, native separators
//! B <unit>                        unit begins (plan unit id)
//! S <dir>                         assembly folder `<dest>.emu-rust-part` about to be created (exclusive)
//! M <n> <from> <to>               intention: file move n of the unit (n = 0, 1, …)
//! m <n>                           file move n done
//! W <from> <to>                   intention: one rename of a whole folder
//! w                               that rename done
//! D+ <dir>                        folder created
//! D- <dir>                        folder removed (emptied by the run)
//! C <unit>                        unit committed (moves done and library rewritten)
//! E                               end of the run (or of its recovery)
//! I                               identity registries rewritten (`emu_runtime::relocate_identities`)
//! U                               run undone
//! ```
//!
//! Every path but the root is relative to the root, components joined by `/` on every platform; each component
//! is percent-encoded byte by byte (`%XX`, upper-case hex) except `A-Z a-z 0-9 - . _ ~ ( ) [ ] ! , + & '`, so a
//! field never holds a space, a `%`, a separator or a control character. A decoded component must not be empty,
//! `.`, `..`, nor contain `/`, `\` (Windows) or NUL: a journal can never name a path outside the root. The root is
//! percent-encoded as a whole, with `/`, `\` and `:` kept. Fields are separated by one space; `<unit>` and `<n>`
//! are decimal `u32`.
//!
//! Reading: at most [`MAX_JOURNAL_SIZE`] bytes; a last line without `\n` is a write cut by a crash and is dropped
//! (`Journal::torn`); anything else that does not parse makes the whole journal unreadable (error with the line
//! number, never its content).

// The writer and most encoders are used by the execution, undo and recovery (FB1b); tests cover them meanwhile.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "used by the tidy execution (FB1b)")
)]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use super::TidyJournalInfo;
use crate::LibraryError;

pub(crate) const MAGIC: &str = "emu-rust tidy 1";
/// Read bound (design v7.1 §3.4).
pub(crate) const MAX_JOURNAL_SIZE: u64 = 16 * 1024 * 1024;
const PREFIX: &str = "journal-";
const SUFFIX: &str = ".txt";

/// One record after the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    Begin(u32),
    Part(PathBuf),
    Move { n: u32, from: PathBuf, to: PathBuf },
    MoveDone(u32),
    Whole { from: PathBuf, to: PathBuf },
    WholeDone,
    DirCreated(PathBuf),
    DirRemoved(PathBuf),
    Commit(u32),
    End,
    Identities,
    Undone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Journal {
    /// Canonical roms root of the run (compared with the current one before any replay).
    pub(crate) root: PathBuf,
    pub(crate) ops: Vec<Op>,
    /// The last line had no `\n` (crash while writing it): it was dropped.
    pub(crate) torn: bool,
}

fn journal_error(path: &Path, message: impl Into<String>) -> LibraryError {
    LibraryError::Journal {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

fn kept(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~()[]!,+&'".contains(&b)
}

fn escape(bytes: &[u8], also: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if kept(b) || also.contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn unescape(field: &str) -> Option<Vec<u8>> {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = field.get(i + 1..i + 3)?;
                if !hex
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
                {
                    return None;
                }
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b if kept(b) => {
                out.push(b);
                i += 1;
            }
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(unix)]
fn os_bytes(s: &std::ffi::OsStr) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    Some(s.as_bytes().to_vec())
}

#[cfg(not(unix))]
fn os_bytes(s: &std::ffi::OsStr) -> Option<Vec<u8>> {
    s.to_str().map(|s| s.as_bytes().to_vec())
}

#[cfg(unix)]
fn os_string(bytes: Vec<u8>) -> Option<std::ffi::OsString> {
    use std::os::unix::ffi::OsStringExt;
    Some(std::ffi::OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn os_string(bytes: Vec<u8>) -> Option<std::ffi::OsString> {
    String::from_utf8(bytes).ok().map(Into::into)
}

/// `None` for a path that is empty, absolute or holds anything but plain names (`..`, `.`, a drive prefix).
fn encode_rel(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for c in path.components() {
        let Component::Normal(name) = c else {
            return None;
        };
        parts.push(escape(&os_bytes(name)?, b""));
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn decode_rel(field: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in field.split('/') {
        let bytes = unescape(part)?;
        let bad = bytes.is_empty()
            || bytes == b"."
            || bytes == b".."
            || bytes.contains(&b'/')
            || bytes.contains(&0)
            || (cfg!(windows) && (bytes.contains(&b'\\') || bytes.contains(&b':')));
        if bad {
            return None;
        }
        path.push(os_string(bytes)?);
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

/// The two header lines.
pub(crate) fn header(root: &Path) -> Option<String> {
    if !root.is_absolute() {
        return None;
    }
    let root = escape(&os_bytes(root.as_os_str())?, b"/\\:");
    Some(format!("{MAGIC}\nR {root}\n"))
}

/// One line, `\n` included; `None` for a path that cannot be journaled (absolute, `..`, non-UTF-8 on Windows).
pub(crate) fn encode(op: &Op) -> Option<String> {
    let line = match op {
        Op::Begin(u) => format!("B {u}"),
        Op::Part(dir) => format!("S {}", encode_rel(dir)?),
        Op::Move { n, from, to } => format!("M {n} {} {}", encode_rel(from)?, encode_rel(to)?),
        Op::MoveDone(n) => format!("m {n}"),
        Op::Whole { from, to } => format!("W {} {}", encode_rel(from)?, encode_rel(to)?),
        Op::WholeDone => "w".to_owned(),
        Op::DirCreated(dir) => format!("D+ {}", encode_rel(dir)?),
        Op::DirRemoved(dir) => format!("D- {}", encode_rel(dir)?),
        Op::Commit(u) => format!("C {u}"),
        Op::End => "E".to_owned(),
        Op::Identities => "I".to_owned(),
        Op::Undone => "U".to_owned(),
    };
    Some(line + "\n")
}

fn parse_op(line: &str) -> Option<Op> {
    let fields: Vec<&str> = line.split(' ').collect();
    let num = |s: &str| -> Option<u32> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    Some(match fields.as_slice() {
        ["B", u] => Op::Begin(num(u)?),
        ["S", d] => Op::Part(decode_rel(d)?),
        ["M", n, from, to] => Op::Move {
            n: num(n)?,
            from: decode_rel(from)?,
            to: decode_rel(to)?,
        },
        ["m", n] => Op::MoveDone(num(n)?),
        ["W", from, to] => Op::Whole {
            from: decode_rel(from)?,
            to: decode_rel(to)?,
        },
        ["w"] => Op::WholeDone,
        ["D+", d] => Op::DirCreated(decode_rel(d)?),
        ["D-", d] => Op::DirRemoved(decode_rel(d)?),
        ["C", u] => Op::Commit(num(u)?),
        ["E"] => Op::End,
        ["I"] => Op::Identities,
        ["U"] => Op::Undone,
        _ => return None,
    })
}

/// Parses a whole journal; the error is a French message without any of the file's content.
pub(crate) fn parse(bytes: &[u8]) -> Result<Journal, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "texte non UTF-8".to_owned())?;
    let torn = !text.is_empty() && !text.ends_with('\n');
    let complete = match text.rfind('\n') {
        Some(end) => &text[..end],
        None => "",
    };
    let mut lines = complete.split('\n');
    if lines.next() != Some(MAGIC) {
        return Err("en-tête absent ou version inconnue".to_owned());
    }
    let root = lines
        .next()
        .and_then(|l| l.strip_prefix("R "))
        .and_then(unescape_root)
        .filter(|r| r.is_absolute())
        .ok_or_else(|| "racine absente ou invalide (ligne 2)".to_owned())?;
    let mut ops = Vec::new();
    for (index, line) in lines.enumerate() {
        ops.push(parse_op(line).ok_or_else(|| format!("ligne {} invalide", index + 3))?);
    }
    Ok(Journal { root, ops, torn })
}

fn unescape_root(field: &str) -> Option<PathBuf> {
    // Separators and the drive colon are kept as is by `header`.
    let mut bytes = Vec::with_capacity(field.len());
    for piece in field.split_inclusive(['/', '\\', ':']) {
        let (body, sep) = match piece.char_indices().last() {
            Some((i, c @ ('/' | '\\' | ':'))) => (&piece[..i], Some(c as u8)),
            _ => (piece, None),
        };
        bytes.extend(unescape(body)?);
        bytes.extend(sep);
    }
    if bytes.contains(&0) {
        return None;
    }
    os_string(bytes).map(PathBuf::from)
}

/// Reads and parses `path` (at most [`MAX_JOURNAL_SIZE`] bytes).
pub(crate) fn read(path: &Path) -> Result<Journal, LibraryError> {
    let io = |source| LibraryError::Io {
        path: path.to_path_buf(),
        source,
    };
    let file = File::open(path).map_err(io)?;
    if !file.metadata().map_err(io)?.is_file() {
        return Err(journal_error(path, "pas un fichier ordinaire"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_JOURNAL_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_JOURNAL_SIZE {
        return Err(journal_error(path, "fichier trop gros"));
    }
    parse(&bytes).map_err(|m| journal_error(path, m))
}

/// `journal-<unix>.txt` / `journal-<unix>-<k>.txt` → `(unix, k)`.
pub(crate) fn parse_name(name: &str) -> Option<(i64, u32)> {
    let body = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    let (unix, k) = match body.split_once('-') {
        Some((u, k)) => (u, k.parse().ok()?),
        None => (body, 0),
    };
    if unix.is_empty() || !unix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((unix.parse().ok()?, k))
}

pub(crate) fn file_name(unix: i64, k: u32) -> String {
    if k == 0 {
        format!("{PREFIX}{unix}{SUFFIX}")
    } else {
        format!("{PREFIX}{unix}-{k}{SUFFIX}")
    }
}

/// Journals of `dir`, oldest first, with their creation time; a missing folder has none.
pub(crate) fn list(dir: &Path) -> Result<Vec<(PathBuf, i64)>, LibraryError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(LibraryError::Io {
                path: dir.to_path_buf(),
                source,
            });
        }
    };
    let mut found: Vec<((i64, u32), PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| Some((parse_name(e.file_name().to_str()?)?, e.path())))
        .collect();
    found.sort();
    Ok(found
        .into_iter()
        .map(|((unix, _), path)| (path, unix))
        .collect())
}

impl Journal {
    pub(crate) fn info(&self, path: PathBuf, created_unix: i64) -> TidyJournalInfo {
        let closing = self
            .ops
            .iter()
            .rposition(|op| matches!(op, Op::End | Op::Undone));
        let identities_done = self
            .ops
            .iter()
            .enumerate()
            .any(|(i, op)| *op == Op::Identities && closing.is_some_and(|c| i > c));
        TidyJournalInfo {
            path,
            created_unix,
            moved: self.committed_moves(),
            undone: self.ops.contains(&Op::Undone),
            complete: self.ops.contains(&Op::End),
            identities_done,
        }
    }

    /// Moves done (`m`, `w`) by the committed units of the run (records before its first `E`).
    pub(crate) fn committed_moves(&self) -> u64 {
        let mut total = 0;
        let mut current = 0;
        for op in &self.ops {
            match op {
                Op::Begin(_) => current = 0,
                Op::MoveDone(_) | Op::WholeDone => current += 1,
                Op::Commit(_) => total += std::mem::take(&mut current),
                Op::End => break,
                _ => {}
            }
        }
        total
    }
}

/// Appends records to a journal, each followed by `sync_all`.
pub(crate) struct JournalWriter {
    file: File,
    path: PathBuf,
}

impl JournalWriter {
    /// Creates `<dir>/journal-<unix>[-k].txt` (never an existing file) and writes the header, synced.
    pub(crate) fn create(dir: &Path, root: &Path, unix: i64) -> Result<Self, LibraryError> {
        let header =
            header(root).ok_or_else(|| journal_error(root, "racine non absolue ou non UTF-8"))?;
        fs::create_dir_all(dir).map_err(|source| LibraryError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        for k in 0..1000 {
            let path = dir.join(file_name(unix, k));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    let mut writer = JournalWriter { file, path };
                    writer.write(&header)?;
                    return Ok(writer);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(LibraryError::Io { path, source }),
            }
        }
        Err(journal_error(dir, "trop de journaux pour la même seconde"))
    }

    /// Opens an existing journal to append to it (recovery, undo, `I` marker).
    pub(crate) fn append_to(path: &Path) -> Result<Self, LibraryError> {
        let file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|source| LibraryError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(JournalWriter {
            file,
            path: path.to_path_buf(),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn append(&mut self, op: &Op) -> Result<(), LibraryError> {
        let line = encode(op)
            .ok_or_else(|| journal_error(&self.path, "chemin impossible à journaliser"))?;
        self.write(&line)
    }

    fn write(&mut self, text: &str) -> Result<(), LibraryError> {
        let io = |source| LibraryError::Io {
            path: self.path.clone(),
            source,
        };
        self.file.write_all(text.as_bytes()).map_err(io)?;
        self.file.sync_all().map_err(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        s.split('/').collect()
    }

    fn all_ops() -> Vec<Op> {
        vec![
            Op::Begin(0),
            Op::DirCreated(p("psx")),
            Op::Part(p("psx/Game (Europe).emu-rust-part")),
            Op::Move {
                n: 0,
                from: p("Game (Europe).cue"),
                to: p("psx/Game (Europe)/Game (Europe).cue"),
            },
            Op::MoveDone(0),
            Op::Move {
                n: 1,
                from: p("Game (Europe) (Track 1).bin"),
                to: p("psx/Game (Europe)/Game (Europe) (Track 1).bin"),
            },
            Op::MoveDone(1),
            Op::Commit(0),
            Op::Begin(7),
            Op::Whole {
                from: p("vrac/Doom"),
                to: p("dos/Doom"),
            },
            Op::WholeDone,
            Op::DirRemoved(p("vrac")),
            Op::Commit(7),
            Op::End,
            Op::Identities,
            Op::Undone,
        ]
    }

    fn text(root: &Path, ops: &[Op]) -> String {
        let mut t = header(root).unwrap();
        for op in ops {
            t.push_str(&encode(op).unwrap());
        }
        t
    }

    fn abs_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\Jo Ë\emu rust\roms")
        } else {
            PathBuf::from("/home/jo Ë/emu rust/roms")
        }
    }

    #[test]
    fn every_record_round_trips() {
        let root = abs_root();
        let t = text(&root, &all_ops());
        let j = parse(t.as_bytes()).unwrap();
        assert_eq!(j.root, root);
        assert_eq!(j.ops, all_ops());
        assert!(!j.torn);
    }

    #[test]
    fn escaping_keeps_fields_free_of_spaces_and_separators() {
        let line = encode(&Op::Move {
            n: 3,
            from: p("a b%c/é\u{7}.st"),
            to: p("atarist/a b%c/é\u{7}.st"),
        })
        .unwrap();
        assert_eq!(
            line,
            "M 3 a%20b%25c/%C3%A9%07.st atarist/a%20b%25c/%C3%A9%07.st\n"
        );
        assert_eq!(line.matches(' ').count(), 3);
        let j = parse(text(&abs_root(), &[]).as_bytes()).unwrap();
        assert!(j.ops.is_empty());
        let header = header(&abs_root()).unwrap();
        assert!(header.starts_with("emu-rust tidy 1\nR "));
        assert!(
            !header["emu-rust tidy 1\nR ".len()..]
                .trim_end()
                .contains(' ')
        );
    }

    #[test]
    fn unsafe_paths_are_never_written_nor_read() {
        for bad in ["../x", "/abs", "a/../b", "./a", ""] {
            assert_eq!(encode(&Op::Part(PathBuf::from(bad))), None, "{bad}");
        }
        assert_eq!(header(Path::new("relative/roms")), None);
        let base = header(&abs_root()).unwrap();
        for bad in [
            "S ..",
            "S a/../b",
            "S %2E%2E",
            "S a/%2F/b",
            "S a//b",
            "S a%00b",
            "S a%zz",
            "S a%2f",
            "S a b",
            "M 1 a",
            "M x a b",
            "m -1",
            "B 99999999999",
            "X",
            "E extra",
            "",
        ] {
            let t = format!("{base}{bad}\n");
            assert!(parse(t.as_bytes()).is_err(), "{bad:?}");
        }
        assert!(parse(b"emu-rust tidy 2\nR /x\n").is_err());
        assert!(parse(b"emu-rust tidy 1\nR relative\n").is_err());
        assert!(parse(b"emu-rust tidy 1\n").is_err());
        assert!(parse(b"").is_err());
        assert!(parse(b"emu-rust tidy 1\nR /x\n\xff\n").is_err());
    }

    #[test]
    fn error_messages_never_quote_the_content() {
        let t = format!("{}B secret-name\n", header(&abs_root()).unwrap());
        let message = parse(t.as_bytes()).unwrap_err();
        assert_eq!(message, "ligne 3 invalide");
    }

    /// A crash may cut the journal after any byte: every prefix ending on a line boundary parses to the records
    /// written so far, a prefix cut inside a line drops that line (and only it).
    #[test]
    fn a_truncated_journal_keeps_its_complete_lines() {
        let root = abs_root();
        let ops = all_ops();
        let full = text(&root, &ops);
        let header_len = header(&root).unwrap().len();
        let mut boundaries = vec![header_len];
        let mut at = header_len;
        for op in &ops {
            at += encode(op).unwrap().len();
            boundaries.push(at);
        }
        for cut in header_len..=full.len() {
            let j = parse(&full.as_bytes()[..cut]).unwrap();
            let complete = boundaries.iter().filter(|&&b| b <= cut).count() - 1;
            assert_eq!(j.ops, ops[..complete], "cut at {cut}");
            assert_eq!(j.torn, !boundaries.contains(&cut), "cut at {cut}");
        }
        // Cut inside the header: unreadable, never guessed.
        assert!(parse(&full.as_bytes()[..header_len - 1]).is_err());
    }

    #[test]
    fn names_and_listing() {
        assert_eq!(
            parse_name("journal-1700000000.txt"),
            Some((1_700_000_000, 0))
        );
        assert_eq!(
            parse_name("journal-1700000000-2.txt"),
            Some((1_700_000_000, 2))
        );
        for bad in [
            "journal-.txt",
            "journal-12a.txt",
            "journal-1.txt.bak",
            "lock",
            "journal--1.txt",
        ] {
            assert_eq!(parse_name(bad), None, "{bad}");
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(list(&dir.path().join("absent")).unwrap().is_empty());
        let root = dir.path().canonicalize().unwrap();
        let a = JournalWriter::create(dir.path(), &root, 20).unwrap();
        let b = JournalWriter::create(dir.path(), &root, 20).unwrap();
        let c = JournalWriter::create(dir.path(), &root, 3).unwrap();
        fs::write(dir.path().join("lock"), b"").unwrap();
        let listed: Vec<PathBuf> = list(dir.path())
            .unwrap()
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(
            listed,
            vec![
                c.path().to_path_buf(),
                a.path().to_path_buf(),
                b.path().to_path_buf()
            ]
        );
        assert!(b.path().ends_with("journal-20-1.txt"));
    }

    #[test]
    fn writer_appends_synced_lines_and_info_reads_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let mut w = JournalWriter::create(&dir.path().join("rangement"), &root, 42).unwrap();
        for op in &all_ops()[..14] {
            w.append(op).unwrap();
        }
        assert!(w.append(&Op::Part(PathBuf::from("../escape"))).is_err());
        let path = w.path().to_path_buf();
        drop(w);
        let j = read(&path).unwrap();
        assert_eq!(j.root, root);
        let info = j.info(path.clone(), 42);
        assert_eq!(info.moved, 3);
        assert!(info.complete && !info.undone && !info.identities_done);
        let mut w = JournalWriter::append_to(&path).unwrap();
        w.append(&Op::Identities).unwrap();
        let info = read(&path).unwrap().info(path.clone(), 42);
        assert!(info.identities_done);
        w.append(&Op::Undone).unwrap();
        let info = read(&path).unwrap().info(path.clone(), 42);
        assert!(info.undone && !info.identities_done);
        w.append(&Op::Identities).unwrap();
        assert!(read(&path).unwrap().info(path.clone(), 42).identities_done);
    }

    #[test]
    fn an_oversized_journal_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal-1.txt");
        let mut t = header(&abs_root()).unwrap();
        while (t.len() as u64) <= MAX_JOURNAL_SIZE {
            t.push_str("E\n");
        }
        fs::write(&path, t).unwrap();
        assert!(matches!(read(&path), Err(LibraryError::Journal { .. })));
    }
}
