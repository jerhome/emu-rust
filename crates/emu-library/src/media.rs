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
    if first != "disk" && first != "disc" {
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

fn ext_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Groups files of the same folder and extension whose names differ only by a disk marker (`(Disk N of M)`,
/// `(Disk N)`, `(Side A|B)`, trailing `Disk N`; case-insensitive). Returns the groups (two files or more) and
/// the other files, in input order. `.m3u` files are never grouped: one named like a group becomes its
/// `playlist` (and is not returned among the others). A group is cut at [`MAX_MEDIA_IMAGES`] members.
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
    }

    #[test]
    fn m3u_parsing_matches_the_runtime_rules() {
        let dir = Path::new("/r/st");
        let ok = parse_m3u(
            "\u{feff}#EXTM3U\n\nA.st|Disk A\r\nsub\\B.st\n./C.st\n /abs/D.st \n|only label\n",
            dir,
        )
        .unwrap();
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
}
