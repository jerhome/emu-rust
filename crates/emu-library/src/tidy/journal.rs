//! Tidy journal, format version 1 (ADR 0023 §4.5 and §4.8, design v7.1 §3.4.1 which is authoritative). Written
//! before anything moves; read by the recovery, the undo, the identity step and « Annuler le dernier rangement ».
//! This module encodes, parses (strict grammar), lists and appends; `exec.rs` gives the lines their meaning.
//!
//! File: `<journal_dir>/journal-<unix>.txt` (`journal-<unix>-<k>.txt` when that name is taken), UTF-8 text, one
//! record per line ending with `\n`, never rewritten in place (only cut back to its last `\n` before appending),
//! `sync_all` after every write:
//!
//! ```text
//! emu-rust tidy 1                               header (exact)
//! R <root>                                      canonical roms root, absolute, native separators
//! B <unit>                                      block begins (plan unit id)
//! D+ <dir>                                      intention: folder created (only by the block that creates it)
//! D- <dir>                                      intention: folder removed
//! S <dir>                                       intention: assembly folder `<D>.emu-rust-part` created (exclusive)
//! M <n> <size> <mtime> <from> <to>              intention: file move n (hard link, then the source is removed)
//! m <n>                                         link n made
//! W <files> <size> <mtime> <from> <to>          intention: one rename of a whole folder (aggregate of its files)
//! w                                             that rename done
//! C <unit>                                      block committed (moves done and library rewritten)
//! E                                             end of the run (or of its recovery)
//! U                                             end of the undo section (or of its recovery)
//! I                                             identity registries rewritten (`emu_runtime::relocate_identities`)
//! ```
//!
//! Grammar (anything else refuses the whole journal, error with the line number, never its content):
//! `header [forward [undo]]`, `forward = {block} [E [I]]`, `undo = {block} [U [I]]` (only after `E`),
//! `block = B u {body} [C u]`. A block without its `C` is open (abandoned at the next `B`, `E`, `U` or the end).
//! In a block with `S <D>.emu-rust-part`, the `M` destinations and the `D+` after `S` name final paths under `<D>/`
//! (the `D+` before `S` may create the ancestors of `<D>`). Undo blocks carry the exact inverse of the moves of a
//! committed forward block (same `n`, paths swapped, same size / file count). A move whose two paths are equal
//! after case and accent folding is refused (the recovery would take them for two copies of one file). Every
//! prefix of a valid journal cut at a line end is valid.
//!
//! Every path but the root is relative to the root, components joined by `/` on every platform; each component
//! is percent-encoded byte by byte (`%XX`, upper-case hex) except `A-Z a-z 0-9 - . _ ~ ( ) [ ] ! , + & '`, so a
//! field never holds a space, a `%`, a separator or a control character. A decoded component must not be empty,
//! `.`, `..`, nor contain `/` or NUL (nor, on Windows, `\`, `:`, a reserved device name or a trailing dot / space):
//! a journal can never name a path outside the root. The root is percent-encoded as a whole, with `/`, `\` and `:`
//! kept. Numbers are canonical decimal (ASCII digits, no sign, no leading zero but `0`).
//!
//! Reading: at most [`MAX_JOURNAL_SIZE`] bytes; a last line without `\n` is a write cut by a crash and is dropped.
//! An empty file or a cut header (fewer than two complete lines, content a prefix of the header) means « nothing
//! moved » ([`read`] returns `None`).

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use super::TidyJournalInfo;
use super::plan::PART_SUFFIX;
use super::sys::Sys;
use crate::LibraryError;

pub(crate) const MAGIC: &str = "emu-rust tidy 1";
/// Read bound (design v7.1 §3.4.1).
pub(crate) const MAX_JOURNAL_SIZE: u64 = 16 * 1024 * 1024;
/// A forward section is closed before the file would exceed this, so its undo always fits under the read bound.
pub(crate) const MAX_FORWARD_SIZE: u64 = 7 * 1024 * 1024;
/// Bound of the undo section (room left for `U` and `I`).
pub(crate) const MAX_UNDO_SIZE: u64 = MAX_JOURNAL_SIZE - 64;
/// « Same state as the line »: FAT 2 s grain, Windows caches, network shares.
pub(crate) const MTIME_TOLERANCE_SECS: i64 = 2;
const PREFIX: &str = "journal-";
const SUFFIX: &str = ".txt";

/// One record after the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    Begin(u32),
    Part(PathBuf),
    Move {
        n: u32,
        size: u64,
        mtime: i64,
        from: PathBuf,
        to: PathBuf,
    },
    MoveDone(u32),
    Whole {
        files: u32,
        size: u64,
        mtime: i64,
        from: PathBuf,
        to: PathBuf,
    },
    WholeDone,
    DirCreated(PathBuf),
    DirRemoved(PathBuf),
    Commit(u32),
    End,
    Identities,
    Undone,
}

/// One `B u … [C u]` block: its body lines in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) unit: u32,
    pub(crate) body: Vec<Op>,
    pub(crate) committed: bool,
}

impl Block {
    /// `S` folder of the block, if any.
    pub(crate) fn part(&self) -> Option<&Path> {
        self.body.iter().find_map(|op| match op {
            Op::Part(p) => Some(p.as_path()),
            _ => None,
        })
    }

    /// `<D>` of the `S <D>.emu-rust-part` line.
    pub(crate) fn final_dir(&self) -> Option<PathBuf> {
        self.part().and_then(final_of_part)
    }

    pub(crate) fn is_done(&self, done: &Op) -> bool {
        self.body.contains(done)
    }

    /// `(from, to)` of the `M` and `W` lines, in order.
    pub(crate) fn pairs(&self) -> Vec<(PathBuf, PathBuf)> {
        self.body
            .iter()
            .filter_map(|op| match op {
                Op::Move { from, to, .. } | Op::Whole { from, to, .. } => {
                    Some((from.clone(), to.clone()))
                }
                _ => None,
            })
            .collect()
    }
}

/// The state of a journal (design v7.1 §3.4.1, table of states).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// 1: forward section without `E`.
    Running,
    /// 2: `… E`, identities due.
    Ended,
    /// 3: `… E I`.
    EndedMarked,
    /// 4: `E` written, undo section opened, no `U`.
    Undoing,
    /// 5: `… U`, identities due.
    Undone,
    /// 6: `… U I`.
    UndoneMarked,
}

impl State {
    pub(crate) fn in_progress(self) -> bool {
        matches!(self, State::Running | State::Undoing)
    }

    /// Last line `E` or `U`: the identity step is due.
    pub(crate) fn identities_due(self) -> bool {
        matches!(self, State::Ended | State::Undone)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Journal {
    /// Canonical roms root of the run (compared with the current one before any action).
    pub(crate) root: PathBuf,
    pub(crate) forward: Vec<Block>,
    /// Undo section (blocks after `E`).
    pub(crate) undo: Vec<Block>,
    pub(crate) state: State,
    /// The last line had no `\n` (crash while writing it): it was dropped.
    pub(crate) torn: bool,
    /// Bytes of the complete lines (header included).
    pub(crate) len: u64,
}

fn journal_error(path: &Path, message: impl Into<String>) -> LibraryError {
    LibraryError::Journal {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

/// A journal that could not be written: an I/O error, not an unreadable journal (RV3 6).
fn write_error(path: &Path, message: &str) -> LibraryError {
    LibraryError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::other(message.to_owned()),
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

/// A component Windows would not keep as written (M2): trailing dot or space, a reserved device name (with or
/// without extension), a character it forbids in names. Checked on every platform by the tests.
pub(crate) fn bad_windows_component(bytes: &[u8]) -> bool {
    if bytes.ends_with(b".") || bytes.ends_with(b" ") {
        return true;
    }
    if bytes
        .iter()
        .any(|&b| b < 0x20 || b"<>:\"|?*\\".contains(&b))
    {
        return true;
    }
    let stem = bytes.split(|&b| b == b'.').next().unwrap_or(bytes);
    let stem = String::from_utf8_lossy(stem)
        .trim_end()
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && stem.as_bytes()[3].is_ascii_digit()
        && stem.as_bytes()[3] != b'0')
}

fn is_combining(c: char) -> bool {
    matches!(u32::from(c), 0x300..=0x36F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

fn base_letter(c: char) -> char {
    match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

/// Case and accent folding of a relative path (RV1 I1): lower case, combining marks of the listed blocks dropped,
/// the accented Latin-1 / Latin Extended-A letters of [`base_letter`] reduced to their base letter, `\` read as `/`.
/// It is incomplete (FV1a m9): letters such as `ř ť ł ș`, Cyrillic, Greek or Vietnamese precomposed letters, the
/// kana marks U+3099 / U+309A and the NTFS `$UpCase` table are not folded, so two names one volume treats as the same
/// may fold differently there; the creation itself then fails (`AlreadyExists`), nothing is overwritten. Where it
/// does fold, it is coarser than NFC (`Pokémon.gb` and `Pokemon.gb` fold equal): equal folds are refused, never
/// merged, which is the safe direction.
pub(crate) fn fold_path(path: &Path) -> String {
    let mut out = String::new();
    for c in path.to_string_lossy().chars() {
        let c = if c == '\\' { '/' } else { c };
        if is_combining(c) {
            continue;
        }
        for l in c.to_lowercase() {
            out.push(base_letter(l));
        }
    }
    out
}

/// `true` when `a` and `b` may name one file on a case-insensitive or normalizing file system.
pub(crate) fn same_place(a: &Path, b: &Path) -> bool {
    fold_path(a) == fold_path(b)
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
            || (cfg!(windows) && bad_windows_component(&bytes));
        if bad {
            return None;
        }
        path.push(os_string(bytes)?);
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

/// `<D>` of `<D>.emu-rust-part`; `None` when the last component does not end with the suffix or is only it.
pub(crate) fn final_of_part(part: &Path) -> Option<PathBuf> {
    let name = part.file_name()?.to_str()?;
    let stem = name.strip_suffix(PART_SUFFIX)?;
    if stem.is_empty() {
        return None;
    }
    Some(part.with_file_name(stem))
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
        Op::Move {
            n,
            size,
            mtime,
            from,
            to,
        } => format!(
            "M {n} {size} {} {} {}",
            (*mtime).max(0),
            encode_rel(from)?,
            encode_rel(to)?
        ),
        Op::MoveDone(n) => format!("m {n}"),
        Op::Whole {
            files,
            size,
            mtime,
            from,
            to,
        } => format!(
            "W {files} {size} {} {} {}",
            (*mtime).max(0),
            encode_rel(from)?,
            encode_rel(to)?
        ),
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

/// Canonical decimal: ASCII digits, no sign, no leading zero (but `0` itself).
fn canonical<T: std::str::FromStr>(s: &str) -> Option<T> {
    let ok =
        s == "0" || (!s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()));
    ok.then(|| s.parse().ok()).flatten()
}

fn parse_op(line: &str) -> Option<Op> {
    let fields: Vec<&str> = line.split(' ').collect();
    Some(match fields.as_slice() {
        ["B", u] => Op::Begin(canonical(u)?),
        ["S", d] => Op::Part(decode_rel(d)?),
        ["M", n, size, mtime, from, to] => Op::Move {
            n: canonical(n)?,
            size: canonical(size)?,
            mtime: canonical(mtime)?,
            from: decode_rel(from)?,
            to: decode_rel(to)?,
        },
        ["m", n] => Op::MoveDone(canonical(n)?),
        ["W", files, size, mtime, from, to] => Op::Whole {
            files: canonical(files)?,
            size: canonical(size)?,
            mtime: canonical(mtime)?,
            from: decode_rel(from)?,
            to: decode_rel(to)?,
        },
        ["w"] => Op::WholeDone,
        ["D+", d] => Op::DirCreated(decode_rel(d)?),
        ["D-", d] => Op::DirRemoved(decode_rel(d)?),
        ["C", u] => Op::Commit(canonical(u)?),
        ["E"] => Op::End,
        ["I"] => Op::Identities,
        ["U"] => Op::Undone,
        _ => return None,
    })
}

/// Where the validator is between two lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Forward,
    AfterE,
    AfterEI,
    Undo,
    AfterU,
    AfterUI,
}

/// Checks of the open block.
struct Open {
    block: Block,
    /// `n` → link written.
    moves: HashMap<u32, bool>,
    /// `W` seen → rename written.
    whole: Option<bool>,
    final_dir: Option<PathBuf>,
}

impl Open {
    fn new(unit: u32) -> Self {
        Open {
            block: Block {
                unit,
                body: Vec::new(),
                committed: false,
            },
            moves: HashMap::new(),
            whole: None,
            final_dir: None,
        }
    }
}

/// Strictly under `dir` (not `dir` itself).
fn strictly_under(path: &Path, dir: &Path) -> bool {
    path.strip_prefix(dir)
        .is_ok_and(|rest| !rest.as_os_str().is_empty())
}

/// A `D+` of a block with `S`: under `<D>/`, or a strict ancestor of `<D>` the block created (RV1 I4).
fn dir_allowed(dir: &Path, final_dir: &Path) -> bool {
    strictly_under(dir, final_dir) || strictly_under(final_dir, dir)
}

struct Validator {
    phase: Phase,
    forward: Vec<Block>,
    undo: Vec<Block>,
    open: Option<Open>,
    seen: HashSet<u32>,
    /// Committed forward blocks by unit (index in `forward`).
    committed: HashMap<u32, usize>,
}

impl Validator {
    fn close_open(&mut self) {
        if let Some(open) = self.open.take() {
            match self.phase {
                Phase::Undo => self.undo.push(open.block),
                _ => self.forward.push(open.block),
            }
        }
    }

    fn forward_of(&self, unit: u32) -> Option<&Block> {
        self.committed.get(&unit).map(|&i| &self.forward[i])
    }

    fn begin(&mut self, u: u32) -> Option<()> {
        match self.phase {
            Phase::Forward | Phase::Undo => {}
            Phase::AfterE | Phase::AfterEI => {
                self.phase = Phase::Undo;
                self.seen.clear();
            }
            Phase::AfterU | Phase::AfterUI => return None,
        }
        self.close_open();
        if !self.seen.insert(u) {
            return None;
        }
        if self.phase == Phase::Undo && !self.committed.contains_key(&u) {
            return None;
        }
        self.open = Some(Open::new(u));
        Some(())
    }

    fn commit(&mut self, u: u32) -> Option<()> {
        let open = self.open.as_ref()?;
        if open.block.unit != u
            || open.moves.values().any(|done| !done)
            || open.whole == Some(false)
        {
            return None;
        }
        if self.phase == Phase::Undo {
            let forward = self.forward_of(u)?;
            let forward_moves: HashSet<u32> = forward
                .body
                .iter()
                .filter_map(|op| match op {
                    Op::Move { n, .. } => Some(*n),
                    _ => None,
                })
                .collect();
            let undo_moves: HashSet<u32> = open.moves.keys().copied().collect();
            let forward_whole = forward.body.iter().any(|op| matches!(op, Op::Whole { .. }));
            if forward_moves != undo_moves || forward_whole != open.whole.is_some() {
                return None;
            }
        }
        let mut open = self.open.take()?;
        open.block.committed = true;
        if self.phase == Phase::Undo {
            self.undo.push(open.block);
        } else {
            self.committed.insert(u, self.forward.len());
            self.forward.push(open.block);
        }
        Some(())
    }

    fn body(&mut self, body: Op) -> Option<()> {
        let undo = match self.phase {
            Phase::Forward => false,
            Phase::Undo => true,
            _ => return None,
        };
        let forward = match &self.open {
            Some(open) if undo => Some(self.forward_of(open.block.unit)?.clone()),
            _ => None,
        };
        let open = self.open.as_mut()?;
        let inverse_of =
            |pred: &dyn Fn(&Op) -> bool| forward.as_ref().is_none_or(|f| f.body.iter().any(pred));
        match &body {
            Op::DirCreated(d) => {
                if open.final_dir.as_ref().is_some_and(|f| !dir_allowed(d, f)) {
                    return None;
                }
            }
            Op::DirRemoved(_) => {}
            Op::Part(p) => {
                if undo
                    || open.final_dir.is_some()
                    || open.whole.is_some()
                    || !open.moves.is_empty()
                {
                    return None;
                }
                let final_dir = final_of_part(p)?;
                let earlier_ok = open.block.body.iter().all(|op| match op {
                    Op::DirCreated(d) => dir_allowed(d, &final_dir),
                    _ => true,
                });
                if !earlier_ok {
                    return None;
                }
                open.final_dir = Some(final_dir);
            }
            Op::Move {
                n, size, from, to, ..
            } => {
                if open.whole.is_some() || open.moves.contains_key(n) || same_place(from, to) {
                    return None;
                }
                if open
                    .final_dir
                    .as_ref()
                    .is_some_and(|f| !strictly_under(to, f))
                {
                    return None;
                }
                let inverse = inverse_of(&|op: &Op| {
                    matches!(op, Op::Move { n: fn_, size: fs_, from: ff, to: ft, .. }
                        if fn_ == n && fs_ == size && ff == to && ft == from)
                });
                if !inverse {
                    return None;
                }
                open.moves.insert(*n, false);
            }
            Op::MoveDone(n) => match open.moves.get_mut(n) {
                Some(done @ false) => *done = true,
                _ => return None,
            },
            Op::Whole {
                files,
                size,
                from,
                to,
                ..
            } => {
                if open.final_dir.is_some()
                    || open.whole.is_some()
                    || !open.moves.is_empty()
                    || same_place(from, to)
                {
                    return None;
                }
                let inverse = inverse_of(&|op: &Op| {
                    matches!(op, Op::Whole { files: ff_, size: fs_, from: ff, to: ft, .. }
                        if ff_ == files && fs_ == size && ff == to && ft == from)
                });
                if !inverse {
                    return None;
                }
                open.whole = Some(false);
            }
            Op::WholeDone => match &mut open.whole {
                Some(done @ false) => *done = true,
                _ => return None,
            },
            _ => return None,
        }
        open.block.body.push(body);
        Some(())
    }

    fn line(&mut self, op: Op) -> Option<()> {
        match op {
            Op::Begin(u) => self.begin(u),
            Op::Commit(u) => self.commit(u),
            Op::End => {
                if self.phase != Phase::Forward {
                    return None;
                }
                self.close_open();
                self.phase = Phase::AfterE;
                Some(())
            }
            Op::Undone => {
                if !matches!(self.phase, Phase::AfterE | Phase::AfterEI | Phase::Undo) {
                    return None;
                }
                self.close_open();
                self.phase = Phase::AfterU;
                Some(())
            }
            Op::Identities => {
                self.phase = match self.phase {
                    Phase::AfterE => Phase::AfterEI,
                    Phase::AfterU => Phase::AfterUI,
                    _ => return None,
                };
                Some(())
            }
            body => self.body(body),
        }
    }
}

/// `true` for an empty file or a cut header: « nothing moved » (FV0 11).
fn is_cut_header(bytes: &[u8]) -> bool {
    if bytes.iter().filter(|&&b| b == b'\n').count() >= 2 {
        return false;
    }
    let first = format!("{MAGIC}\n");
    if bytes.len() <= first.len() {
        return first.as_bytes().starts_with(bytes);
    }
    let Some(rest) = bytes.strip_prefix(first.as_bytes()) else {
        return false;
    };
    // `R …` without its `\n` (a second `\n` would make two complete lines, excluded above).
    b"R ".starts_with(&rest[..rest.len().min(2)])
}

/// Parses a whole journal; `Ok(None)`: empty or cut header. The error is a French message without any of the
/// file's content.
pub(crate) fn parse(bytes: &[u8]) -> Result<Option<Journal>, String> {
    if is_cut_header(bytes) {
        return Ok(None);
    }
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
    let mut v = Validator {
        phase: Phase::Forward,
        forward: Vec::new(),
        undo: Vec::new(),
        open: None,
        seen: HashSet::new(),
        committed: HashMap::new(),
    };
    for (index, line) in lines.enumerate() {
        let invalid = || format!("ligne {} invalide", index + 3);
        let op = parse_op(line).ok_or_else(invalid)?;
        v.line(op).ok_or_else(invalid)?;
    }
    let state = match v.phase {
        Phase::Forward => State::Running,
        Phase::AfterE => State::Ended,
        Phase::AfterEI => State::EndedMarked,
        Phase::Undo => State::Undoing,
        Phase::AfterU => State::Undone,
        Phase::AfterUI => State::UndoneMarked,
    };
    v.close_open();
    Ok(Some(Journal {
        root,
        forward: v.forward,
        undo: v.undo,
        state,
        torn,
        len: complete.len() as u64 + 1,
    }))
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

fn read_bytes(path: &Path) -> Result<Vec<u8>, LibraryError> {
    let io = |source| LibraryError::Io {
        path: path.to_path_buf(),
        source,
    };
    let meta = fs::symlink_metadata(path).map_err(io)?;
    if !meta.is_file() {
        return Err(journal_error(path, "pas un fichier ordinaire"));
    }
    let file = File::open(path).map_err(io)?;
    let mut bytes = Vec::new();
    file.take(MAX_JOURNAL_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_JOURNAL_SIZE {
        return Err(journal_error(path, "fichier trop gros"));
    }
    Ok(bytes)
}

/// Reads and parses `path` (at most [`MAX_JOURNAL_SIZE`] bytes); `Ok(None)`: empty or cut header.
pub(crate) fn read(path: &Path) -> Result<Option<Journal>, LibraryError> {
    let bytes = read_bytes(path)?;
    parse(&bytes).map_err(|m| journal_error(path, m))
}

/// `journal-<unix>.txt` / `journal-<unix>-<k>.txt` (k ≥ 1) → `(unix, k)`; canonical digits only (M3).
pub(crate) fn parse_name(name: &str) -> Option<(i64, u32)> {
    let body = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    let (unix, k) = match body.split_once('-') {
        Some((u, k)) => {
            let k: u32 = canonical(k)?;
            if k == 0 {
                return None;
            }
            (u, k)
        }
        None => (body, 0),
    };
    Some((canonical(unix)?, k))
}

pub(crate) fn file_name(unix: i64, k: u32) -> String {
    if k == 0 {
        format!("{PREFIX}{unix}{SUFFIX}")
    } else {
        format!("{PREFIX}{unix}-{k}{SUFFIX}")
    }
}

/// One journal file of the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) path: PathBuf,
    pub(crate) unix: i64,
    pub(crate) k: u32,
}

/// Journals of `dir`, oldest first (`(unix, k)` order); a missing folder has none.
pub(crate) fn list(dir: &Path) -> Result<Vec<Listed>, LibraryError> {
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
    let mut found: Vec<Listed> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let (unix, k) = parse_name(e.file_name().to_str()?)?;
            Some(Listed {
                path: e.path(),
                unix,
                k,
            })
        })
        .collect();
    found.sort_by_key(|l| (l.unix, l.k));
    Ok(found)
}

/// What a listed file holds.
#[derive(Debug)]
pub(crate) enum Content {
    /// Empty or cut header: « nothing moved ».
    Empty,
    Unreadable(LibraryError),
    Journal(Box<Journal>),
}

/// Every journal of `dir`, oldest first, read.
pub(crate) fn load_all(dir: &Path) -> Result<Vec<(Listed, Content)>, LibraryError> {
    Ok(list(dir)?
        .into_iter()
        .map(|l| {
            let content = match read(&l.path) {
                Ok(None) => Content::Empty,
                Ok(Some(j)) => Content::Journal(Box::new(j)),
                Err(e) => Content::Unreadable(e),
            };
            (l, content)
        })
        .collect())
}

impl Journal {
    pub(crate) fn info(&self, path: PathBuf, created_unix: i64) -> TidyJournalInfo {
        TidyJournalInfo {
            path,
            created_unix,
            moved: self.committed_moves(),
            undone: matches!(self.state, State::Undone | State::UndoneMarked),
            complete: !self.state.in_progress(),
            identities_done: matches!(self.state, State::EndedMarked | State::UndoneMarked),
            switch: None,
        }
    }

    /// Moves (`M` / `W`) of the committed blocks of the forward section.
    pub(crate) fn committed_moves(&self) -> u64 {
        self.forward
            .iter()
            .filter(|b| b.committed)
            .map(|b| b.pairs().len() as u64)
            .sum()
    }

    /// Net moves `(stale, current)`, relative (design v7.1 §3.4.1): for every committed forward unit, its pairs
    /// as written when it was not brought back by a committed undo block, swapped otherwise.
    pub(crate) fn net_moves(&self) -> Vec<(PathBuf, PathBuf)> {
        let back: HashSet<u32> = self
            .undo
            .iter()
            .filter(|b| b.committed)
            .map(|b| b.unit)
            .collect();
        let mut out = Vec::new();
        for block in self.forward.iter().filter(|b| b.committed) {
            for (from, to) in block.pairs() {
                if back.contains(&block.unit) {
                    out.push((to, from));
                } else {
                    out.push((from, to));
                }
            }
        }
        out
    }

    /// Blocks of the section in progress (the forward one in state 1, the undo one in state 4).
    pub(crate) fn current_section(&self) -> &[Block] {
        match self.state {
            State::Undoing => &self.undo,
            _ => &self.forward,
        }
    }
}

/// Appends records to a journal, every write followed by `sync_all`.
pub(crate) struct JournalWriter {
    file: File,
    path: PathBuf,
    len: u64,
    poisoned: bool,
}

impl JournalWriter {
    /// Creates `<dir>/journal-<unix>[-k].txt` (never an existing file), syncs the folder (I4), writes the
    /// header, synced. `dir` must exist.
    pub(crate) fn create(
        sys: &dyn Sys,
        dir: &Path,
        root: &Path,
        unix: i64,
    ) -> Result<Self, LibraryError> {
        let header =
            header(root).ok_or_else(|| write_error(root, "racine non absolue ou non UTF-8"))?;
        for k in 0..1000 {
            let path = dir.join(file_name(unix, k));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    sys.sync_dir(dir).map_err(|source| LibraryError::Io {
                        path: dir.to_path_buf(),
                        source,
                    })?;
                    let mut writer = JournalWriter {
                        file,
                        path,
                        len: 0,
                        poisoned: false,
                    };
                    writer.write(sys, &header)?;
                    return Ok(writer);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(LibraryError::Io { path, source }),
            }
        }
        Err(write_error(dir, "trop de journaux pour la même seconde"))
    }

    /// Opens an existing journal to append to it: first cuts it back to its last `\n` and syncs (I1), so a torn
    /// line is never glued to the next record. No `\n` is ever added to « repair » a line.
    pub(crate) fn append_to(sys: &dyn Sys, path: &Path) -> Result<Self, LibraryError> {
        let io = |source| LibraryError::Io {
            path: path.to_path_buf(),
            source,
        };
        let bytes = read_bytes(path)?;
        let len = bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i as u64 + 1);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(io)?;
        if len != bytes.len() as u64 {
            sys.truncate_synced(&file, len).map_err(io)?;
        }
        file.seek(SeekFrom::Start(len)).map_err(io)?;
        Ok(JournalWriter {
            file,
            path: path.to_path_buf(),
            len,
            poisoned: false,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Bytes written so far (header included).
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// Appends `ops` in one write and one `sync_all`.
    pub(crate) fn append(&mut self, sys: &dyn Sys, ops: &[Op]) -> Result<(), LibraryError> {
        let mut text = String::new();
        for op in ops {
            text.push_str(
                &encode(op)
                    .ok_or_else(|| write_error(&self.path, "chemin impossible à journaliser"))?,
            );
        }
        self.write(sys, &text)
    }

    fn write(&mut self, sys: &dyn Sys, text: &str) -> Result<(), LibraryError> {
        if self.poisoned {
            return Err(write_error(&self.path, "écriture du journal interrompue"));
        }
        if let Err(source) = sys.write_synced(&mut self.file, text.as_bytes()) {
            // A failed write may have left a torn line: this writer never writes again; the next one cuts the
            // file back to its last `\n` first (I1).
            self.poisoned = true;
            return Err(LibraryError::Io {
                path: self.path.clone(),
                source,
            });
        }
        self.len += text.len() as u64;
        Ok(())
    }
}

/// Encoded size of `ops`, `None` when a path cannot be journaled.
pub(crate) fn encoded_len(ops: &[Op]) -> Option<u64> {
    ops.iter()
        .map(|op| encode(op).map(|l| l.len() as u64))
        .sum()
}

#[cfg(test)]
mod tests;
