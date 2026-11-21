//! Logical order of the floppies found in one zip (`Disk N` / `Side X`), as the library groups them.

use crate::content::ArchiveEntry;

/// Disk extensions of the computers loading several floppies.
/// Must match emu-library `DISK_EXTENSIONS` (the crates cannot depend on each other).
const DISK_EXTENSIONS: &[&str] = &[
    "dsk", "d64", "d71", "d81", "g64", "nib", "adf", "adz", "dms", "fdi", "ipf", "st", "msa",
    "dim", "stx",
];

/// Orders the entries of one zip when it holds two floppies or more and nothing else: by `(Disk N)`,
/// `(Disk N of M)`, `(Side A|B)` or trailing `Disk N` when every name carries a distinct number and they
/// differ only by it; otherwise alphabetically, case ignored. Anything else keeps the archive order.
///
/// Must match emu-library `zip_disk_order` (scan.rs, `group_media`): the library names the game after this
/// order, so the first disk started here must be the one it expects. The "one computer system" check of the
/// library is not reproduced; the extension list is the same.
pub(crate) fn sort_zip_disks(entries: &mut [ArchiveEntry]) {
    if entries.len() < 2 || !entries.iter().all(|e| is_disk(&e.name)) {
        return;
    }
    let marked: Option<Vec<(String, String, u32)>> = entries
        .iter()
        .map(|e| {
            let (stem, ext) = e.name.rsplit_once('.')?;
            let m = find_marker(stem)?;
            let base = tidy(format!("{}{}", &stem[..m.start], &stem[m.end..]));
            (!base.is_empty()).then(|| (base.to_lowercase(), ext.to_ascii_lowercase(), m.number))
        })
        .collect();
    if let Some(keys) = marked
        && keys.iter().all(|k| k.0 == keys[0].0 && k.1 == keys[0].1)
        && {
            let mut numbers: Vec<u32> = keys.iter().map(|k| k.2).collect();
            numbers.sort_unstable();
            numbers.windows(2).all(|w| w[0] != w[1])
        }
    {
        let mut order: Vec<(u32, ArchiveEntry)> = keys
            .iter()
            .map(|k| k.2)
            .zip(entries.iter().cloned())
            .collect();
        order.sort_by_key(|(n, _)| *n);
        for (slot, (_, e)) in entries.iter_mut().zip(order) {
            *slot = e;
        }
        return;
    }
    entries.sort_by_key(|e| e.name.to_lowercase());
}

fn is_disk(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| DISK_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

struct Marker {
    start: usize,
    end: usize,
    number: u32,
}

fn parse_inner(inner: &str) -> Option<u32> {
    let lower = inner.trim().to_ascii_lowercase();
    let mut words = lower.split_whitespace();
    let first = words.next()?;
    if first == "side" {
        let n = match words.next()? {
            "a" => 1,
            "b" => 2,
            _ => return None,
        };
        return words.next().is_none().then_some(n);
    }
    if first != "disk" && first != "disc" && first != "tape" {
        return None;
    }
    let number: u32 = words.next()?.parse().ok().filter(|n| *n > 0)?;
    match words.next() {
        None => Some(number),
        Some("of") => {
            words.next()?.parse::<u32>().ok()?;
            words.next().is_none().then_some(number)
        }
        Some(_) => None,
    }
}

/// First parenthesised marker, else a trailing `Disk N` (same grammar as emu-library `find_marker`).
fn find_marker(stem: &str) -> Option<Marker> {
    let mut open = None;
    for (i, c) in stem.char_indices() {
        match c {
            '(' => open = Some(i),
            ')' => {
                if let Some(start) = open.take()
                    && let Some(number) = parse_inner(&stem[start + 1..i])
                {
                    return Some(Marker {
                        start,
                        end: i + 1,
                        number,
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
    if head[..word_start]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric())
    {
        return None;
    }
    Some(Marker {
        start: word_start,
        end: trimmed.len(),
        number,
    })
}

fn tidy(mut name: String) -> String {
    while name.contains("  ") {
        name = name.replace("  ", " ");
    }
    name.trim_matches(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(names: &[&str]) -> Vec<ArchiveEntry> {
        names
            .iter()
            .enumerate()
            .map(|(index, n)| ArchiveEntry {
                index,
                name: (*n).to_owned(),
                declared: 1,
            })
            .collect()
    }

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut e = entries(names);
        sort_zip_disks(&mut e);
        e.into_iter().map(|e| e.name).collect()
    }

    #[test]
    fn disk_numbers_beat_archive_order() {
        assert_eq!(
            sorted(&[
                "B.A.T. (1991)(Ubi Soft)(Disk 3 of 4).dsk",
                "B.A.T. (1991)(Ubi Soft)(Disk 1 of 4).dsk",
                "B.A.T. (1991)(Ubi Soft)(Disk 4 of 4).dsk",
                "B.A.T. (1991)(Ubi Soft)(Disk 2 of 4).dsk",
            ]),
            [
                "B.A.T. (1991)(Ubi Soft)(Disk 1 of 4).dsk",
                "B.A.T. (1991)(Ubi Soft)(Disk 2 of 4).dsk",
                "B.A.T. (1991)(Ubi Soft)(Disk 3 of 4).dsk",
                "B.A.T. (1991)(Ubi Soft)(Disk 4 of 4).dsk",
            ]
        );
        assert_eq!(
            sorted(&["g disk 10.adf", "g disk 2.adf"]),
            ["g disk 2.adf", "g disk 10.adf"]
        );
    }

    #[test]
    fn unnumbered_names_are_alphabetical_ignoring_case() {
        assert_eq!(
            sorted(&["b.st", "C.st", "A.msa"]),
            ["A.msa", "b.st", "C.st"]
        );
        // One name without marker: no numeric grouping.
        assert_eq!(
            sorted(&["G (Disk 2).st", "G.st", "G (Disk 1).st"]),
            ["G (Disk 1).st", "G (Disk 2).st", "G.st"]
        );
    }

    #[test]
    fn sides_a_then_b() {
        assert_eq!(
            sorted(&["Maniac (Side B).dsk", "Maniac (Side A).dsk"]),
            ["Maniac (Side A).dsk", "Maniac (Side B).dsk"]
        );
    }

    #[test]
    fn other_content_keeps_archive_order() {
        assert_eq!(sorted(&["b.prg", "a.st"]), ["b.prg", "a.st"]);
        assert_eq!(sorted(&["only.st"]), ["only.st"]);
    }
}
