//! clrmamepro DAT (No-Intro, libretro-database `metadat/no-intro/`). Untrusted text: bounded, no panic.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::LibraryError;

/// Larger DAT files are refused (the biggest 1.0 DAT is a few MiB).
pub const MAX_DAT_SIZE: u64 = 64 * 1024 * 1024;

/// Nesting deeper than this is refused (real files use 2 levels); bounds the parser's recursion.
const MAX_DEPTH: usize = 8;
/// Key/value items in one top-level block, nested blocks included (a real `game` block has about 15). The tree of
/// a single top-level block is the only one held in memory, so this bounds the parser's memory whatever the nesting.
const MAX_BLOCK_NODES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatEntry {
    /// `game ( name "…" )`: the No-Intro title, also the libretro-thumbnails file name.
    pub game: String,
    pub rom_name: String,
    pub size: u64,
    pub crc32: u32,
    pub sha1: Option<[u8; 20]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dat {
    /// `clrmamepro ( name … version … )` header.
    pub name: String,
    pub version: Option<String>,
    pub entries: Vec<DatEntry>,
}

/// Reads a DAT file from disk: at most `MAX_DAT_SIZE` bytes, UTF-8 (an optional BOM is dropped), then
/// `parse_clrmamepro`.
pub fn load_dat_file(path: &Path) -> Result<Dat, LibraryError> {
    let io = |source| LibraryError::Io {
        path: path.to_path_buf(),
        source,
    };
    let file = File::open(path).map_err(io)?;
    let mut bytes = Vec::new();
    file.take(MAX_DAT_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_DAT_SIZE {
        return Err(LibraryError::TooLarge {
            path: path.to_path_buf(),
        });
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(e) => {
            let line = 1 + bytes[..e.valid_up_to()]
                .iter()
                .filter(|&&b| b == b'\n')
                .count();
            return Err(dat_error(line, "texte non UTF-8"));
        }
    };
    parse_clrmamepro(text.strip_prefix('\u{FEFF}').unwrap_or(text))
}

fn dat_error(line: usize, message: &str) -> LibraryError {
    LibraryError::Dat {
        line,
        message: message.to_owned(),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Open,
    Close,
    /// Bare word or quoted string (escapes resolved).
    Text(std::borrow::Cow<'a, str>),
}

struct Lexer<'a> {
    text: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    /// `Ok(None)` at end of input. The returned line is the token's first line.
    fn next(&mut self) -> Result<Option<(Token<'a>, usize)>, LibraryError> {
        let bytes = self.text.as_bytes();
        while let Some(&b) = bytes.get(self.pos) {
            if b == b'\n' {
                self.line += 1;
            } else if !b.is_ascii_whitespace() {
                break;
            }
            self.pos += 1;
        }
        let Some(&b) = bytes.get(self.pos) else {
            return Ok(None);
        };
        let line = self.line;
        match b {
            b'(' => {
                self.pos += 1;
                Ok(Some((Token::Open, line)))
            }
            b')' => {
                self.pos += 1;
                Ok(Some((Token::Close, line)))
            }
            b'"' => self.quoted().map(|t| Some((t, line))),
            _ => {
                let start = self.pos;
                while let Some(&c) = bytes.get(self.pos) {
                    if c.is_ascii_whitespace() || matches!(c, b'(' | b')' | b'"') {
                        break;
                    }
                    self.pos += 1;
                }
                // Boundaries are ASCII bytes, hence char boundaries.
                let word = self.text.get(start..self.pos).unwrap_or_default();
                Ok(Some((Token::Text(word.into()), line)))
            }
        }
    }

    /// Strings are single-line; `\"` and `\\` are escapes, any other backslash is literal (Windows-like names).
    fn quoted(&mut self) -> Result<Token<'a>, LibraryError> {
        let bytes = self.text.as_bytes();
        let start = self.pos + 1;
        let mut i = start;
        let mut escaped = false;
        loop {
            match bytes.get(i) {
                None | Some(b'\n') | Some(b'\r') => {
                    return Err(dat_error(self.line, "chaîne non terminée"));
                }
                Some(b'"') => break,
                Some(b'\\') if matches!(bytes.get(i + 1), Some(b'"' | b'\\')) => {
                    escaped = true;
                    i += 2;
                }
                Some(_) => i += 1,
            }
        }
        self.pos = i + 1;
        let raw = self.text.get(start..i).unwrap_or_default();
        if !escaped {
            return Ok(Token::Text(raw.into()));
        }
        let mut out = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\'
                && let Some(&next @ ('"' | '\\')) = chars.peek()
            {
                out.push(next);
                chars.next();
            } else {
                out.push(c);
            }
        }
        Ok(Token::Text(out.into()))
    }
}

enum Node<'a> {
    Atom(std::borrow::Cow<'a, str>),
    Block(Vec<(std::borrow::Cow<'a, str>, Node<'a>)>),
}

impl<'a> Node<'a> {
    fn get(&self, key: &str) -> Option<&Node<'a>> {
        match self {
            Node::Block(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            Node::Atom(_) => None,
        }
    }

    fn atom(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Node::Atom(s) => Some(s),
            Node::Block(_) => None,
        }
    }
}

/// Value after a key: an atom, or a block whose `(` has not been consumed yet. `budget` is the number of items
/// the current top-level block may still hold.
fn parse_value<'a>(
    lexer: &mut Lexer<'a>,
    depth: usize,
    key_line: usize,
    budget: &mut usize,
) -> Result<Node<'a>, LibraryError> {
    match lexer.next()? {
        None => Err(dat_error(key_line, "valeur manquante")),
        Some((Token::Close, line)) => Err(dat_error(line, "valeur manquante")),
        Some((Token::Text(t), _)) => Ok(Node::Atom(t)),
        Some((Token::Open, line)) => parse_block(lexer, depth + 1, line, budget),
    }
}

fn parse_block<'a>(
    lexer: &mut Lexer<'a>,
    depth: usize,
    open_line: usize,
    budget: &mut usize,
) -> Result<Node<'a>, LibraryError> {
    if depth > MAX_DEPTH {
        return Err(dat_error(open_line, "imbrication trop profonde"));
    }
    let mut items = Vec::new();
    loop {
        match lexer.next()? {
            None => return Err(dat_error(open_line, "bloc non fermé")),
            Some((Token::Close, _)) => return Ok(Node::Block(items)),
            Some((Token::Open, line)) => return Err(dat_error(line, "clé attendue")),
            Some((Token::Text(key), line)) => {
                let Some(left) = budget.checked_sub(1) else {
                    return Err(dat_error(line, "bloc trop long"));
                };
                *budget = left;
                let value = parse_value(lexer, depth, line, budget)?;
                items.push((key, value));
            }
        }
    }
}

/// Consumes the value after an ignored key without building it: same syntax checks as `parse_value`, no memory.
fn skip_value(lexer: &mut Lexer<'_>, key_line: usize) -> Result<(), LibraryError> {
    let open_line = match lexer.next()? {
        None => return Err(dat_error(key_line, "valeur manquante")),
        Some((Token::Close, line)) => return Err(dat_error(line, "valeur manquante")),
        Some((Token::Text(_), _)) => return Ok(()),
        Some((Token::Open, line)) => line,
    };
    // Each open block expects a key, then a value; `opens` holds the line of every unclosed `(`.
    let mut opens = vec![open_line];
    let mut want_value = false;
    while let Some(&innermost) = opens.last() {
        match (lexer.next()?, want_value) {
            (None, _) => return Err(dat_error(innermost, "bloc non fermé")),
            (Some((Token::Close, line)), true) => return Err(dat_error(line, "valeur manquante")),
            (Some((Token::Close, _)), false) => {
                opens.pop();
            }
            (Some((Token::Open, line)), false) => return Err(dat_error(line, "clé attendue")),
            (Some((Token::Open, line)), true) => {
                if opens.len() >= MAX_DEPTH {
                    return Err(dat_error(line, "imbrication trop profonde"));
                }
                opens.push(line);
                want_value = false;
                continue;
            }
            (Some((Token::Text(_), _)), want) => {
                want_value = !want;
                continue;
            }
        }
        want_value = false;
    }
    Ok(())
}

fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (o, pair) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
        let s = std::str::from_utf8(pair).ok()?;
        *o = u8::from_str_radix(s, 16).ok()?;
    }
    Some(out)
}

/// One `rom ( … )` of a game; `None` when `name`, `size` or `crc` is missing or invalid.
fn rom_entry(game: &str, rom: &Node<'_>) -> Option<DatEntry> {
    let rom_name = rom.atom("name")?;
    let size_text = rom.atom("size")?;
    // `u64::from_str` also accepts a leading `+`; the DAT format does not.
    if !size_text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let size = size_text.parse::<u64>().ok()?;
    let crc32 = u32::from_be_bytes(parse_hex::<4>(rom.atom("crc")?)?);
    let sha1 = rom.atom("sha1").and_then(parse_hex::<20>);
    Some(DatEntry {
        game: game.to_owned(),
        rom_name: rom_name.to_owned(),
        size,
        crc32,
        sha1,
    })
}

/// Unknown blocks and keys are skipped; a `rom` without valid `size` and `crc` is skipped with a warning; broken
/// syntax (unterminated string or block) is `LibraryError::Dat`.
pub fn parse_clrmamepro(text: &str) -> Result<Dat, LibraryError> {
    if text.len() as u64 > MAX_DAT_SIZE {
        return Err(dat_error(0, "fichier trop gros"));
    }
    let mut lexer = Lexer {
        text,
        pos: 0,
        line: 1,
    };
    let mut dat = Dat::default();
    let mut skipped = 0usize;
    let mut first_skipped_line = 0usize;
    loop {
        let (key, line) = match lexer.next()? {
            None => break,
            Some((Token::Text(key), line)) => (key, line),
            Some((Token::Open, line)) => return Err(dat_error(line, "clé attendue")),
            Some((Token::Close, line)) => {
                return Err(dat_error(line, "parenthèse fermante en trop"));
            }
        };
        if !matches!(&*key, "clrmamepro" | "game" | "machine") {
            skip_value(&mut lexer, line)?;
            continue;
        }
        // Only the tree of the current top-level block is held, and `MAX_BLOCK_NODES` bounds it whatever its
        // nesting: the parser's memory stays bounded (the entries kept are bounded by `MAX_DAT_SIZE`).
        let mut budget = MAX_BLOCK_NODES;
        let node = parse_value(&mut lexer, 0, line, &mut budget)?;
        match &*key {
            "clrmamepro" => {
                if let Some(name) = node.atom("name") {
                    dat.name = name.to_owned();
                }
                dat.version = node.atom("version").map(str::to_owned);
            }
            "game" | "machine" => {
                let Node::Block(items) = &node else {
                    continue;
                };
                let game = node.atom("name");
                for (k, v) in items {
                    if k != "rom" {
                        continue;
                    }
                    match game.and_then(|g| rom_entry(g, v)) {
                        Some(entry) => dat.entries.push(entry),
                        None => {
                            if skipped == 0 {
                                first_skipped_line = line;
                            }
                            skipped += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if skipped > 0 {
        // Counts and line numbers only: DAT contents are never logged.
        tracing::warn!(
            skipped,
            first_line = first_skipped_line,
            "DAT entries without a valid name, size or crc were skipped"
        );
    }
    Ok(dat)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"clrmamepro (
	name "Nintendo - Game Boy"
	description "Nintendo - Game Boy"
	version "20261119-000000"
	homepage No-Intro
)

game (
	name "Alpha Quest (World)"
	description "Alpha Quest (World)"
	region "World"
	rom ( name "Alpha Quest (World).gb" size 32768 crc CBF43926 md5 00000000000000000000000000000000 sha1 A9993E364706816ABA3E25717850C26C9CD0D89D )
)

game (
	name "Beta \"Quoted\" Racer (Europe)"
	rom ( name "Beta.gb" size 9 crc 0badf00d )
	rom ( name "Beta (Alt).gb" size 9 crc 0BADF00E sha1 nothex )
)

resource ( name "ignored" rom ( name "x" size 1 crc 00000000 ) )
"#;

    #[test]
    fn parses_sample() {
        let dat = parse_clrmamepro(SAMPLE).unwrap();
        assert_eq!(dat.name, "Nintendo - Game Boy");
        assert_eq!(dat.version.as_deref(), Some("20261119-000000"));
        assert_eq!(dat.entries.len(), 3);
        let a = &dat.entries[0];
        assert_eq!(a.game, "Alpha Quest (World)");
        assert_eq!(a.rom_name, "Alpha Quest (World).gb");
        assert_eq!((a.size, a.crc32), (32768, 0xCBF4_3926));
        assert_eq!(a.sha1.unwrap()[..3], [0xA9, 0x99, 0x3E]);
        assert_eq!(dat.entries[1].game, "Beta \"Quoted\" Racer (Europe)");
        assert_eq!(dat.entries[1].crc32, 0x0BAD_F00D);
        assert_eq!(dat.entries[2].sha1, None);
    }

    #[test]
    fn empty_and_header_only() {
        assert_eq!(parse_clrmamepro("").unwrap(), Dat::default());
        assert_eq!(parse_clrmamepro("  \n\t").unwrap(), Dat::default());
        let dat = parse_clrmamepro("clrmamepro ( name x )").unwrap();
        assert_eq!(dat.name, "x");
        assert!(dat.entries.is_empty());
    }

    #[test]
    fn invalid_roms_are_skipped() {
        let text = r#"
game ( name "No size" rom ( name "a" crc 00000000 ) )
game ( name "Bad crc" rom ( name "a" size 1 crc 0000000G ) )
game ( name "Short crc" rom ( name "a" size 1 crc 123 ) )
game ( name "Signed" rom ( name "a" size +1 crc 00000000 ) )
game ( name "Negative" rom ( name "a" size -1 crc 00000000 ) )
game ( name "Overflow" rom ( name "a" size 99999999999999999999999 crc 00000000 ) )
game ( name "Block size" rom ( name "a" size ( k 1 ) crc 00000000 ) )
game ( rom ( name "no game name" size 1 crc 00000000 ) )
game ( name "Unicode crc" rom ( name "a" size 1 crc ééééé ) )
game ( name "Ok" rom ( name "a" size 1 crc 00000000 ) )
game atom
"#;
        let dat = parse_clrmamepro(text).unwrap();
        assert_eq!(dat.entries.len(), 1);
        assert_eq!(dat.entries[0].game, "Ok");
    }

    #[test]
    fn broken_syntax_is_an_error_with_a_line() {
        let cases: &[(&str, usize)] = &[
            ("game ( name \"unterminated )", 1),
            ("game (\n name \"a\"\n", 1),
            ("game ( name \"a\" ) )", 1),
            ("\n\ngame ( ( ) )", 3),
            ("game", 1),
            ("game ( name )", 1),
            ("(", 1),
            ("game ( name \"multi\nline\" )", 1),
        ];
        for (text, line) in cases {
            match parse_clrmamepro(text) {
                Err(LibraryError::Dat { line: l, .. }) => assert_eq!(l, *line, "{text:?}"),
                other => panic!("{text:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn deep_nesting_is_refused_without_stack_overflow() {
        let mut text = "a ".to_owned();
        for _ in 0..100_000 {
            text.push_str("k ( ");
        }
        assert!(matches!(
            parse_clrmamepro(&text),
            Err(LibraryError::Dat { .. })
        ));
        let ok = "game ( name g rom ( name r size 1 crc 00000000 x ( y ( z ( a 1 ) ) ) ) )";
        assert_eq!(parse_clrmamepro(ok).unwrap().entries.len(), 1);
    }

    #[test]
    fn long_block_is_refused() {
        let mut text = "game ( ".to_owned();
        for _ in 0..=MAX_BLOCK_NODES {
            text.push_str("k v ");
        }
        text.push(')');
        assert!(parse_clrmamepro(&text).is_err());
    }

    /// Forged DAT: every block is short, but nesting multiplies them. The budget counts the whole top-level block.
    #[test]
    fn nested_short_blocks_share_one_budget() {
        let inner = "k v ".repeat(1000);
        let mut text = "game ( name g ".to_owned();
        for _ in 0..70 {
            text.push_str(&format!("x ( {inner}) "));
        }
        text.push(')');
        assert!(matches!(
            parse_clrmamepro(&text),
            Err(LibraryError::Dat { line: 1, .. })
        ));
        // The budget is per top-level block: many ordinary games are fine.
        let game = "game ( name g rom ( name r size 1 crc 00000000 ) )\n";
        assert_eq!(
            parse_clrmamepro(&game.repeat(MAX_BLOCK_NODES))
                .unwrap()
                .entries
                .len(),
            MAX_BLOCK_NODES
        );
    }

    #[test]
    fn unknown_blocks_are_skipped_without_a_tree() {
        // Over the budget, but never built: skipped.
        let mut text = format!("resource ( {} ", "k v ".repeat(MAX_BLOCK_NODES + 1));
        text.push_str("nested ( a ( b ( c 1 ) ) d ( ) ) e \"s\" )\n");
        text.push_str("game ( name g rom ( name r size 1 crc 00000000 ) )");
        assert_eq!(parse_clrmamepro(&text).unwrap().entries.len(), 1);
        let cases: &[(&str, usize)] = &[
            ("other ( k ( ) ", 1),
            ("other\n( k )", 2),
            ("other ( k ( j ) )", 1),
            ("other ( ( ) )", 1),
            ("other", 1),
            ("other ( k (\n", 1),
        ];
        for (text, line) in cases {
            match parse_clrmamepro(text) {
                Err(LibraryError::Dat { line: l, .. }) => assert_eq!(l, *line, "{text:?}"),
                other => panic!("{text:?}: {other:?}"),
            }
        }
        let mut deep = "other ( ".to_owned();
        for _ in 0..MAX_DEPTH {
            deep.push_str("k ( ");
        }
        deep.push_str(&")".repeat(MAX_DEPTH + 1));
        assert!(parse_clrmamepro(&deep).is_err());
        let mut ok = "other ( ".to_owned();
        for _ in 1..MAX_DEPTH {
            ok.push_str("k ( ");
        }
        ok.push_str(&")".repeat(MAX_DEPTH));
        assert!(parse_clrmamepro(&ok).is_ok(), "{ok}");
    }

    #[test]
    fn escapes_and_backslashes() {
        let text = r#"game ( name "a\\b\c\"d" rom ( name "r\" size 1 crc 00000000 ) )"#;
        // `"r\"` ends with an escaped quote: the string runs to the end of the line, unterminated.
        assert!(matches!(
            parse_clrmamepro(text),
            Err(LibraryError::Dat { line: 1, .. })
        ));
        let text = r#"game ( name "a\\b\c\"d" rom ( name "r\\" size 1 crc 00000000 ) )"#;
        let dat = parse_clrmamepro(text).unwrap();
        assert_eq!(dat.entries[0].game, r#"a\b\c"d"#);
        assert_eq!(dat.entries[0].rom_name, r"r\");
    }

    #[test]
    fn arbitrary_input_never_panics() {
        // Deterministic xorshift: no extra dev-dependency.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let alphabet = b"() \"\\\nab0F\t";
        for len in 0..2000 {
            let text: String = (0..len % 97)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    alphabet[(state % alphabet.len() as u64) as usize] as char
                })
                .collect();
            let _ = parse_clrmamepro(&text);
        }
        let mut sample = SAMPLE.to_owned();
        while !sample.is_empty() {
            let _ = parse_clrmamepro(&sample);
            let cut = sample.len() - 1;
            let cut = (0..=cut)
                .rev()
                .find(|&i| sample.is_char_boundary(i))
                .unwrap();
            sample.truncate(cut);
        }
    }

    #[test]
    fn load_dat_file_checks_encoding_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.dat");
        std::fs::write(&ok, format!("\u{FEFF}{SAMPLE}")).unwrap();
        assert_eq!(load_dat_file(&ok).unwrap().entries.len(), 3);

        let latin1 = dir.path().join("latin1.dat");
        std::fs::write(
            &latin1,
            b"clrmamepro ( name x )\ngame ( name \"Caf\xE9\" )\n",
        )
        .unwrap();
        assert!(matches!(
            load_dat_file(&latin1),
            Err(LibraryError::Dat { line: 2, .. })
        ));

        let big = dir.path().join("big.dat");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_DAT_SIZE + 1).unwrap();
        assert!(matches!(
            load_dat_file(&big),
            Err(LibraryError::TooLarge { .. })
        ));
        assert!(matches!(
            load_dat_file(&dir.path().join("absent.dat")),
            Err(LibraryError::Io { .. })
        ));
    }
}
