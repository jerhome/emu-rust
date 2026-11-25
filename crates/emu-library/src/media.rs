//! Multi-disk grouping (design v2.0 §3.4): `Game (Disk 1 of 2).st` + `Game (Disk 2 of 2).st` = one game.
//! Pure functions: no file access (the scanner reads `.m3u` files itself).

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Same bound as `emu_runtime::MAX_MEDIA_IMAGES`: a group or playlist never lists more images.
pub const MAX_MEDIA_IMAGES: usize = 32;

/// Disks of one game found in one folder, in disk order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaGroup {
    /// File name without the disk marker (`Game (1990)(Pub)`).
    pub name: String,
    /// At least two files, ordered by disk number.
    pub members: Vec<PathBuf>,
    /// A `.m3u` of the input named like the group, if any.
    pub playlist: Option<PathBuf>,
    /// Disk numbers (1-based) absent from the sequence: the group stays playable.
    pub missing: Vec<u32>,
}

struct Marker {
    start: usize,
    end: usize,
    number: u32,
    total: Option<u32>,
}

fn parse_inner(inner: &str) -> Option<(u32, Option<u32>)> {
    let lower = inner.trim().to_ascii_lowercase();
    let mut words = lower.split_whitespace();
    let first = words.next()?;
    if first == "side" {
        let side = words.next()?;
        let n = match side {
            "a" => 1,
            "b" => 2,
            _ => return None,
        };
        return words.next().is_none().then_some((n, None));
    }
    if first != "disk" && first != "disc" && first != "tape" {
        return None;
    }
    let number: u32 = words.next()?.parse().ok()?;
    if number == 0 {
        return None;
    }
    match words.next() {
        None => Some((number, None)),
        Some("of") => {
            let total: u32 = words.next()?.parse().ok()?;
            words.next().is_none().then_some((number, Some(total)))
        }
        Some(_) => None,
    }
}

/// First parenthesised marker `(Disk N [of M])` / `(Side A|B)`, else a trailing `Disk N`.
fn find_marker(stem: &str) -> Option<Marker> {
    let mut open = None;
    for (i, c) in stem.char_indices() {
        match c {
            '(' => open = Some(i),
            ')' => {
                if let Some(start) = open.take()
                    && let Some((number, total)) = parse_inner(&stem[start + 1..i])
                {
                    return Some(Marker {
                        start,
                        end: i + 1,
                        number,
                        total,
                    });
                }
            }
            _ => {}
        }
    }
    let trimmed = stem.trim_end();
    let digits_at = trimmed.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits_at == trimmed.len() {
        return None;
    }
    let number: u32 = trimmed[digits_at..].parse().ok().filter(|n| *n > 0)?;
    let head = trimmed[..digits_at].trim_end();
    let word_start = head.len().checked_sub(4)?;
    if !head.is_char_boundary(word_start) {
        return None;
    }
    let word = head[word_start..].to_ascii_lowercase();
    if word != "disk" && word != "disc" {
        return None;
    }
    // `Disk 1` must be a word of its own: `Hard Disk 1`-like names are fine, `Xdisk 1` is not.
    let before = head[..word_start].chars().next_back();
    if before.is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    Some(Marker {
        start: word_start,
        end: trimmed.len(),
        number,
        total: None,
    })
}

/// Floppy formats for which a bare trailing digit (`MANIACD0.D64`, `MANIACD1.D64`) may number the disks.
const DIGIT_SUFFIX_EXTENSIONS: &[&str] = &["d64", "g64", "d71", "d81", "adf", "dsk", "st", "msa"];
/// Shortest radical (name without the final digit) accepted by the bare-digit rule.
const MIN_RADICAL_CHARS: usize = 3;
/// Words that may end the radical of a group starting at 1 (`Quest D1`, `Quest_Disk2`): without one, `Game1` /
/// `Game2` is more likely two games of a series than two disks.
const DISK_WORDS: &[&str] = &["d", "disk", "disc", "dsk", "diskette", "side"];

/// `Name3` -> (`Name`, 3): a single final digit after a non-digit, radical of at least [`MIN_RADICAL_CHARS`].
fn split_digit_suffix(stem: &str) -> Option<(String, u32)> {
    let last = stem.chars().next_back()?;
    let digit = last.to_digit(10).filter(|_| last.is_ascii_digit())?;
    let head = &stem[..stem.len() - 1];
    if head.chars().next_back().is_none_or(|c| c.is_ascii_digit()) {
        return None;
    }
    let radical = tidy(head.to_owned());
    (radical.chars().count() >= MIN_RADICAL_CHARS).then_some((radical, digit))
}

/// Does the radical end with a disk word of its own (`Quest D`, `Quest_Disk`, `Quest-Side`), or with an
/// uppercase `D` glued to the name (`MANIACD`, `DemoD`)? A longer glued word (`Xdisk`) or a lowercase final `d`
/// (`Blood`, `World`, `Island`) does not count.
fn ends_with_disk_word(radical: &str) -> bool {
    let lower = radical.to_lowercase();
    let word = lower
        .rsplit(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("");
    let last_word = radical
        .rsplit(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("");
    (word.len() < lower.len() && DISK_WORDS.contains(&word))
        || (last_word.chars().count() > 1
            && last_word.ends_with('D')
            && last_word
                .chars()
                .rev()
                .nth(1)
                .is_some_and(char::is_alphabetic))
}

fn tidy(mut name: String) -> String {
    while name.contains("  ") {
        name = name.replace("  ", " ");
    }
    name.trim_matches(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .to_owned()
}

/// `stem` without its disk marker (unchanged when it has none).
pub(crate) fn strip_media_marker(stem: &str) -> String {
    match find_marker(stem) {
        Some(m) => tidy(format!("{}{}", &stem[..m.start], &stem[m.end..])),
        None => stem.to_owned(),
    }
}

/// Name of a group member as `group_media` would name its group: the standard marker removed, else the final
/// digit of a floppy numbered `MANIACD0`. Only for files known to come from a [`MediaGroup`]: on a plain title
/// (`Tekken 3`) the digit is part of the name.
pub(crate) fn strip_group_member_name(stem: &str) -> String {
    match find_marker(stem) {
        Some(_) => strip_media_marker(stem),
        None => split_digit_suffix(stem).map_or_else(|| stem.to_owned(), |(radical, _)| radical),
    }
}

fn ext_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Groups files of the same folder and extension whose names differ only by a disk marker (`(Disk N of M)`,
/// `(Disk N)`, `(Side A|B)`, trailing `Disk N`; case-insensitive). Returns the groups (two files or more) and
/// the other files, in input order. `.m3u` files are never grouped: one named like a group becomes its
/// `playlist` (and is not returned among the others). A group is cut at [`MAX_MEDIA_IMAGES`] members.
///
/// Second rule, floppies only (`.d64 .g64 .d71 .d81 .adf .dsk .st .msa`, files without a marker above): same
/// folder, same extension, same radical of 3 characters or more, and a single final digit (`MANIACD0`,
/// `MANIACD1`). The digits must form a contiguous run without duplicate, starting at 0 (always accepted) or at 1
/// (accepted only when the radical ends with a disk word such as `D`, `Disk`, `Side`: bare `Game1` / `Game2`
/// stay two games; a start at 0 needs no such word, so `Level0` / `Level1` do group, an accepted risk, since a series is far more common than a two-disk game with that naming). Two-digit endings
/// (`Game10`) never match; a gap (`G0`, `G1`, `G3`) cancels the group; a group never has a playlist nor reports
/// missing disks. The group name is the radical as written (`MANIACD`).
pub fn group_media(files: &[PathBuf]) -> (Vec<MediaGroup>, Vec<PathBuf>) {
    type Key = (PathBuf, String, String);
    let mut buckets: BTreeMap<Key, Vec<(u32, Option<u32>, usize)>> = BTreeMap::new();
    let mut names: BTreeMap<Key, String> = BTreeMap::new();
    for (i, path) in files.iter().enumerate() {
        let ext = ext_lower(path);
        if ext == "m3u" {
            continue;
        }
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let Some(marker) = find_marker(&stem) else {
            continue;
        };
        let name = tidy(format!("{}{}", &stem[..marker.start], &stem[marker.end..]));
        if name.is_empty() {
            continue;
        }
        let key = (
            path.parent().map(Path::to_path_buf).unwrap_or_default(),
            ext,
            name.to_lowercase(),
        );
        names.entry(key.clone()).or_insert(name);
        buckets
            .entry(key)
            .or_default()
            .push((marker.number, marker.total, i));
    }
    let mut grouped = vec![false; files.len()];
    let mut consumed_playlists = Vec::new();
    let mut groups: Vec<(usize, MediaGroup)> = Vec::new();
    for (key, mut entries) in buckets {
        // Two files claiming the same disk number: the first one (input order) keeps the slot.
        entries.sort_by_key(|(n, _, i)| (*n, *i));
        entries.dedup_by_key(|(n, _, _)| *n);
        entries.truncate(MAX_MEDIA_IMAGES);
        if entries.len() < 2 {
            continue;
        }
        let name = names.remove(&key).unwrap_or_default();
        let declared = entries.iter().filter_map(|(_, m, _)| *m).max().unwrap_or(0);
        let highest = entries.last().map_or(0, |(n, _, _)| *n);
        let last = declared.max(highest).min(MAX_MEDIA_IMAGES as u32);
        let missing = (1..=last)
            .filter(|n| !entries.iter().any(|(m, _, _)| m == n))
            .collect();
        let first_index = entries.iter().map(|(_, _, i)| *i).min().unwrap_or(0);
        for (_, _, i) in &entries {
            grouped[*i] = true;
        }
        let playlist = files.iter().position(|p| {
            ext_lower(p) == "m3u"
                && p.parent() == Some(key.0.as_path())
                && p.file_stem()
                    .is_some_and(|s| s.to_string_lossy().to_lowercase() == key.2)
        });
        if let Some(p) = playlist {
            consumed_playlists.push(p);
        }
        groups.push((
            first_index,
            MediaGroup {
                name,
                members: entries.iter().map(|(_, _, i)| files[*i].clone()).collect(),
                playlist: playlist.map(|p| files[p].clone()),
                missing,
            },
        ));
    }
    // Bare trailing digit (`MANIACD0` / `MANIACD1`): floppy extensions only, files without a standard marker.
    let mut digit_buckets: BTreeMap<Key, (String, Vec<(u32, usize)>)> = BTreeMap::new();
    for (i, path) in files.iter().enumerate() {
        let ext = ext_lower(path);
        if !DIGIT_SUFFIX_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        if find_marker(&stem).is_some() {
            continue;
        }
        let Some((radical, digit)) = split_digit_suffix(&stem) else {
            continue;
        };
        let key = (
            path.parent().map(Path::to_path_buf).unwrap_or_default(),
            ext,
            radical.to_lowercase(),
        );
        digit_buckets
            .entry(key)
            .or_insert_with(|| (radical, Vec::new()))
            .1
            .push((digit, i));
    }
    for (radical, mut entries) in digit_buckets.into_values() {
        entries.sort_unstable();
        let first = entries[0].0;
        // Contiguous run from 0 or 1 without duplicate (`game1.d64` + `GAME1.d64` is ambiguous: no group).
        let contiguous = entries.windows(2).all(|w| w[1].0 == w[0].0 + 1);
        let allowed = first == 0 || (first == 1 && ends_with_disk_word(&radical));
        if entries.len() < 2 || entries.len() > MAX_MEDIA_IMAGES || !contiguous || !allowed {
            continue;
        }
        let first_index = entries.iter().map(|(_, i)| *i).min().unwrap_or(0);
        for (_, i) in &entries {
            grouped[*i] = true;
        }
        groups.push((
            first_index,
            MediaGroup {
                name: radical,
                members: entries.iter().map(|(_, i)| files[*i].clone()).collect(),
                playlist: None,
                missing: Vec::new(),
            },
        ));
    }
    groups.sort_by_key(|(i, _)| *i);
    let singles = files
        .iter()
        .enumerate()
        .filter(|(i, _)| !grouped[*i] && !consumed_playlists.contains(i))
        .map(|(_, p)| p.clone())
        .collect();
    (groups.into_iter().map(|(_, g)| g).collect(), singles)
}

/// Images listed by a `.m3u` (text already read, ≤ 64 KiB): one path per line, `#` comments and blank lines
/// skipped, a `|label` suffix dropped, paths absolute or relative to `dir`. `..`, control characters and drive or
/// root prefixes on a relative path are refused. Backslashes are separators on every platform but Windows.
///
/// Common strict rule (library and runtime): the file must be UTF-8 (a leading BOM is tolerated; the caller
/// rejects other bytes before this function), and an entry may contain no control character (TAB included);
/// only leading and trailing whitespace around an entry is trimmed.
///
/// Must stay aligned with `parse_playlist` in `emu-runtime/src/media.rs` (the crates cannot depend on each
/// other): a playlist the core accepts must not be flagged here, or the game would be hidden by the scan.
pub(crate) fn parse_m3u(text: &str, dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = line.split('|').next().unwrap_or("").trim();
        if entry.is_empty() {
            continue;
        }
        if entry.chars().any(char::is_control) {
            return Err("caractère de contrôle dans le .m3u".to_owned());
        }
        let entry = if cfg!(windows) {
            PathBuf::from(entry)
        } else {
            PathBuf::from(entry.replace('\\', "/"))
        };
        if entry.components().any(|c| c == Component::ParentDir) {
            return Err(format!("`..` refusé dans le .m3u : {}", entry.display()));
        }
        let path = if entry.is_absolute() {
            entry
        } else {
            if entry
                .components()
                .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
            {
                return Err(format!(
                    "préfixe de lecteur ou racine refusé dans le .m3u : {}",
                    entry.display()
                ));
            }
            dir.join(entry)
        };
        if out.len() >= MAX_MEDIA_IMAGES {
            return Err(format!(".m3u : plus de {MAX_MEDIA_IMAGES} images"));
        }
        out.push(path);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn redump_32x_discs_group_into_one_game() {
        let files = vec![
            p("d/Night Trap (USA) (Disc 2) (Sega CD 32X) (RE-1).cue"),
            p("d/Night Trap (USA) (Disc 1) (Sega CD 32X) (RE-1).cue"),
        ];
        let (groups, singles) = group_media(&files);
        assert!(singles.is_empty());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members.len(), 2);
    }

    #[test]
    fn tosec_names_group_in_disk_order() {
        let files = vec![
            p("d/Game (1990)(Pub)(Disk 2 of 3).st"),
            p("d/Alone (1991)(Pub).st"),
            p("d/Game (1990)(Pub)(Disk 1 of 3).st"),
            p("d/Game (1990)(Pub)(Disk 3 of 3).st"),
        ];
        let (groups, singles) = group_media(&files);
        assert_eq!(singles, [p("d/Alone (1991)(Pub).st")]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Game (1990)(Pub)");
        assert_eq!(
            groups[0].members,
            [
                p("d/Game (1990)(Pub)(Disk 1 of 3).st"),
                p("d/Game (1990)(Pub)(Disk 2 of 3).st"),
                p("d/Game (1990)(Pub)(Disk 3 of 3).st")
            ]
        );
        assert!(groups[0].missing.is_empty());
    }

    #[test]
    fn gaps_are_reported_and_the_group_stays_playable() {
        let files = vec![p("d/G (Disk 1 of 4).st"), p("d/G (Disk 4 of 4).st")];
        let (groups, _) = group_media(&files);
        assert_eq!(groups[0].members.len(), 2);
        assert_eq!(groups[0].missing, [2, 3]);
        // Without a declared total, the highest number present bounds the sequence.
        let files = vec![p("d/G (Disk 1).st"), p("d/G (Disk 3).st")];
        assert_eq!(group_media(&files).0[0].missing, [2]);
    }

    #[test]
    fn sides_trailing_numbers_and_case() {
        let (g, s) = group_media(&[p("d/Demo (Side B).msa"), p("d/demo (SIDE A).msa")]);
        assert!(s.is_empty());
        assert_eq!(
            g[0].members,
            [p("d/demo (SIDE A).msa"), p("d/Demo (Side B).msa")]
        );
        let (g, _) = group_media(&[p("d/Quest Disk 2.st"), p("d/Quest Disk 1.st")]);
        assert_eq!(g[0].name, "Quest");
        assert_eq!(g[0].members[0], p("d/Quest Disk 1.st"));
        let (g, _) = group_media(&[p("d/Quest disk1.st"), p("d/Quest DISK2.st")]);
        assert_eq!(g[0].name, "Quest");
    }

    #[test]
    fn bare_digit_suffix_groups_floppies() {
        let (g, s) = group_media(&[
            p("c64/MANIACD1.D64"),
            p("c64/Other.d64"),
            p("c64/MANIACD0.D64"),
        ]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "MANIACD");
        assert_eq!(g[0].members, [p("c64/MANIACD0.D64"), p("c64/MANIACD1.D64")]);
        assert!(g[0].missing.is_empty() && g[0].playlist.is_none());
        assert_eq!(s, [p("c64/Other.d64")]);
        // Starting at 1 with a disk word, separated or glued; also other floppy formats.
        for files in [
            ["d/Quest D1.adf", "d/Quest D2.adf"],
            ["d/Quest_Disk1.dsk", "d/Quest_Disk2.dsk"],
            ["d/DEMOD1.st", "d/DEMOD2.st"],
        ] {
            let paths: Vec<PathBuf> = files.iter().map(|f| p(f)).collect();
            let (g, s) = group_media(&paths);
            assert_eq!((g.len(), s.len()), (1, 0), "{files:?}");
            assert_eq!(g[0].members, paths);
        }
        let (g, _) = group_media(&[
            p("d/Quest D1.adf"),
            p("d/Quest D2.adf"),
            p("d/Quest D3.adf"),
        ]);
        assert_eq!(g[0].name, "Quest D");
        assert_eq!(strip_group_member_name("MANIACD0"), "MANIACD");
        assert_eq!(strip_media_marker("MANIACD0"), "MANIACD0");
    }

    #[test]
    fn bare_digit_suffix_stays_prudent() {
        for files in [
            // A series, not disks.
            vec!["d/Game1.d64", "d/Game2.d64"],
            vec!["d/Zak1.adf", "d/Zak2.adf"],
            vec!["d/Blood1.adf", "d/Blood2.adf"],
            vec!["d/World1.adf", "d/World2.adf"],
            vec!["d/Island1.d64", "d/Island2.d64"],
            // Radical too short, two-digit endings, gap, duplicate, lone file.
            vec!["d/AB0.d64", "d/AB1.d64"],
            vec!["d/Game10.d64", "d/Game11.d64"],
            vec!["d/GAMED0.d64", "d/GAMED2.d64"],
            vec!["d/GAMED1.d64", "d/gamed1.d64"],
            vec!["d/GAMED0.d64"],
            vec!["d/GAMED2.d64", "d/GAMED3.d64"],
            // Other extensions, folders or extensions mixed, not a floppy.
            vec!["d/GAMED0.nes", "d/GAMED1.nes"],
            vec!["a/GAMED0.d64", "b/GAMED1.d64"],
            vec!["d/GAMED0.d64", "d/GAMED1.adf"],
            // Different games sharing digits.
            vec!["d/Alpha0.d64", "d/Beta1.d64"],
        ] {
            let paths: Vec<PathBuf> = files.iter().map(|f| p(f)).collect();
            let (g, s) = group_media(&paths);
            assert!(g.is_empty(), "{files:?}: {g:?}");
            assert_eq!(s, paths);
        }
    }

    #[test]
    fn standard_markers_win_over_the_digit_rule() {
        let files = [p("d/G (Disk 1 of 2).adf"), p("d/G (Disk 2 of 2).adf")];
        let (g, s) = group_media(&files);
        assert_eq!((g.len(), s.len(), g[0].name.as_str()), (1, 0, "G"));
        let (g, _) = group_media(&[p("d/Quest Disk 1.st"), p("d/Quest Disk 2.st")]);
        assert_eq!(g[0].name, "Quest");
    }

    #[test]
    fn folder_and_extension_separate_groups() {
        let files = vec![
            p("a/G (Disk 1).st"),
            p("b/G (Disk 2).st"),
            p("a/G (Disk 2).msa"),
        ];
        let (groups, singles) = group_media(&files);
        assert!(groups.is_empty());
        assert_eq!(singles, files);
    }

    #[test]
    fn lone_disk_and_non_markers_stay_single() {
        let files = vec![
            p("d/G (Disk 2 of 2).st"),
            p("d/Xdisk 1.st"),
            p("d/Xdisk 2.st"),
            p("d/Hello (Disk A).st"),
            p("d/Hello (Disk B).st"),
            p("d/Disk 7.st"),
        ];
        let (groups, singles) = group_media(&files);
        assert!(groups.is_empty(), "{groups:?}");
        assert_eq!(singles.len(), 6);
    }

    #[test]
    fn m3u_named_like_a_group_is_its_playlist() {
        let files = vec![
            p("d/G.m3u"),
            p("d/G (Disk 1).st"),
            p("d/G (Disk 2).st"),
            p("d/Other.m3u"),
        ];
        let (groups, singles) = group_media(&files);
        assert_eq!(groups[0].playlist, Some(p("d/G.m3u")));
        assert_eq!(singles, [p("d/Other.m3u")]);
    }

    #[test]
    fn duplicate_disk_numbers_and_the_size_cap() {
        let (g, s) = group_media(&[p("d/G (Disk 1).st"), p("d/g (disk 1).st")]);
        assert!(g.is_empty());
        assert_eq!(s.len(), 2);
        let many: Vec<PathBuf> = (1..=40).map(|n| p(&format!("d/G (Disk {n}).st"))).collect();
        let (g, s) = group_media(&many);
        assert_eq!(g[0].members.len(), MAX_MEDIA_IMAGES);
        assert_eq!(s.len(), 8);
    }

    #[test]
    fn marker_is_removed_from_titles() {
        assert_eq!(
            strip_media_marker("Game (1990)(Pub)(Disk 1 of 2)(Cr)"),
            "Game (1990)(Pub)(Cr)"
        );
        assert_eq!(strip_media_marker("Plain (1990)"), "Plain (1990)");
        assert_eq!(strip_media_marker("Quest Disk 1"), "Quest");
        for t in ["Tekken 3", "Silent Hill 2", "Final Fantasy 7"] {
            assert_eq!(strip_media_marker(t), t);
        }
    }

    #[test]
    fn m3u_parsing_matches_the_runtime_rules() {
        let dir = Path::new("/r/st");
        // A rooted path without a drive (`/abs`) is not absolute on Windows and is refused there.
        let abs = if cfg!(windows) {
            "C:\\abs\\D.st"
        } else {
            "/abs/D.st"
        };
        let text =
            format!("\u{feff}#EXTM3U\n\nA.st|Disk A\r\nsub\\B.st\n./C.st\n {abs} \n|only label\n");
        let ok = parse_m3u(&text, dir).unwrap();
        if cfg!(windows) {
            assert_eq!(ok.len(), 4);
        } else {
            assert_eq!(
                ok,
                [
                    p("/r/st/A.st"),
                    p("/r/st/sub/B.st"),
                    p("/r/st/./C.st"),
                    p("/abs/D.st")
                ]
            );
        }
        for bad in [
            "../x.st",
            "a/../../x.st",
            "a\\..\\x.st",
            "a\u{1}b.st",
            "/abs/../x.st|label",
        ] {
            assert!(parse_m3u(bad, dir).is_err(), "{bad}");
        }
        #[cfg(windows)]
        assert!(parse_m3u("C:x.st", dir).is_err());
        let many = "x.st\n".repeat(MAX_MEDIA_IMAGES + 1);
        assert!(parse_m3u(&many, dir).is_err());
    }

    #[test]
    fn sega_cd_and_32x_cd_editions_are_not_grouped() {
        // Same box, different editions (7.4): the tags after the disc number differ, so two games.
        let files: Vec<PathBuf> = [
            "/r/megacd/Fahrenheit (USA) (Disc 1) (Key Disc) (Sega CD).cue",
            "/r/megacd/Fahrenheit (USA) (Disc 2) (32X CD).cue",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
        let (groups, singles) = group_media(&files);
        assert!(groups.is_empty(), "{groups:?}");
        assert_eq!(singles, files);
    }
}
