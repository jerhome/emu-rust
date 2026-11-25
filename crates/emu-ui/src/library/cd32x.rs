//! Sega CD 32X mark of a library game (ADR 0024 §1, §2, §4): derived from the names (`emu-library`) or set by
//! hand in `[library] cd32x` of `config.toml`. Pure logic, tested; the screen only draws and saves.

use emu_library::{Game, Identification};

const MEGA_CD: &str = "megacd";

/// Where the « CD 32X » mark of a game comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cd32xMark {
    No,
    /// A `(Sega CD 32X)`-style tag in the title or in a file name.
    ByName,
    /// The user's choice (`[library] cd32x`).
    Manual,
}

impl Cd32xMark {
    pub(crate) fn is_marked(self) -> bool {
        self != Self::No
    }
}

/// `No` for any game outside `megacd`. `ByName` wins over `Manual`.
pub(crate) fn cd32x_mark(game: &Game, marked: &[String]) -> Cd32xMark {
    if game.system != MEGA_CD {
        return Cd32xMark::No;
    }
    if game.cd32x_by_name() {
        return Cd32xMark::ByName;
    }
    match mark_key(game) {
        Some(key) if marked.iter().any(|m| m.to_lowercase() == key) => Cd32xMark::Manual,
        _ => Cd32xMark::No,
    }
}

/// Key of the manual list: `str::to_lowercase` of the file name of `game.path`; `None` without a UTF-8 name.
pub(crate) fn mark_key(game: &Game) -> Option<String> {
    game.path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_lowercase)
}

/// Adds or removes the key (entries equal after lower-casing are one entry); `true` when the list changed.
pub(crate) fn set_manual_mark(marked: &mut Vec<String>, game: &Game, on: bool) -> bool {
    let Some(key) = mark_key(game) else {
        return false;
    };
    let before = marked.len();
    let present = marked.iter().any(|m| m.to_lowercase() == key);
    if on {
        if present {
            return false;
        }
        marked.push(key);
        true
    } else {
        marked.retain(|m| m.to_lowercase() != key);
        marked.len() != before
    }
}

/// `megacd` games of `games` sharing the key of `game` (itself included): shown when > 1.
pub(crate) fn same_key_count(games: &[Game], game: &Game) -> usize {
    let Some(key) = mark_key(game) else {
        return 0;
    };
    games
        .iter()
        .filter(|g| g.system == MEGA_CD && mark_key(g).as_deref() == Some(key.as_str()))
        .count()
}

/// The help line invites to mark: `megacd`, mark `No`, identification not based on the content (`Unknown` or
/// `Name`, hence every `.chd`).
pub(crate) fn invites_marking(game: &Game, marked: &[String]) -> bool {
    cd32x_mark(game, marked) == Cd32xMark::No
        && game.system == MEGA_CD
        && matches!(
            game.identification,
            Identification::Unknown | Identification::Name
        )
}

/// A game on which « Ce jeu est un CD 32X » / « Ce n'est pas un CD 32X » is offered: `megacd` and not marked by
/// its name (the badge is enough then).
pub(crate) fn can_toggle(game: &Game, marked: &[String]) -> bool {
    game.system == MEGA_CD
        && cd32x_mark(game, marked) != Cd32xMark::ByName
        && mark_key(game).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_library::GameId;

    fn game(system: &str, path: &str, title: &str, id: Identification) -> Game {
        Game {
            id: GameId(1),
            system: system.into(),
            path: path.into(),
            entry: None,
            title: title.into(),
            identification: id,
            size: 1,
            crc32: None,
            sha1: None,
            favorite: false,
            last_played_unix: None,
            play_count: 0,
            media: Vec::new(),
            is_dir: false,
            region: None,
        }
    }

    fn chd(name: &str) -> Game {
        game(
            "megacd",
            &format!("/r/megacd/{name}"),
            "Fake Trap",
            Identification::Name,
        )
    }

    #[test]
    fn mark_comes_from_the_name_then_from_the_list() {
        let none: Vec<String> = Vec::new();
        assert_eq!(cd32x_mark(&chd("Fake Trap.chd"), &none), Cd32xMark::No);
        let tagged = game(
            "megacd",
            "/r/Fake Trap (USA) (Sega CD 32X).chd",
            "Fake Trap",
            Identification::Name,
        );
        assert_eq!(cd32x_mark(&tagged, &none), Cd32xMark::ByName);
        let list = vec!["fake trap.chd".to_owned()];
        assert_eq!(cd32x_mark(&chd("Fake Trap.chd"), &list), Cd32xMark::Manual);
        // The name wins over the list.
        let both = vec!["fake trap (usa) (sega cd 32x).chd".to_owned()];
        assert_eq!(cd32x_mark(&tagged, &both), Cd32xMark::ByName);
        // Another system is never marked, even with the same file name.
        let md = game("megadrive", "/r/Fake Trap.chd", "x", Identification::Name);
        assert_eq!(cd32x_mark(&md, &list), Cd32xMark::No);
        assert!(!can_toggle(&md, &list));
        assert!(!can_toggle(&tagged, &none), "the badge is enough");
        assert!(can_toggle(&chd("Fake Trap.chd"), &none));
    }

    #[test]
    fn hand_written_entries_are_matched_in_unicode_lower_case() {
        let g = chd("Épée Trap (DISC 1).CHD");
        assert_eq!(mark_key(&g).as_deref(), Some("épée trap (disc 1).chd"));
        for entry in ["ÉPÉE TRAP (DISC 1).CHD", "Épée Trap (Disc 1).chd"] {
            assert_eq!(
                cd32x_mark(&g, &[entry.to_owned()]),
                Cd32xMark::Manual,
                "{entry}"
            );
        }
    }

    #[test]
    fn set_mark_adds_removes_and_reports_changes() {
        let g = chd("Fake Trap.chd");
        let mut list = vec!["FAKE TRAP.chd".to_owned(), "other.chd".to_owned()];
        assert!(!set_manual_mark(&mut list, &g, true), "already there");
        assert_eq!(list.len(), 2);
        assert!(set_manual_mark(&mut list, &g, false));
        assert_eq!(list, ["other.chd"]);
        assert!(!set_manual_mark(&mut list, &g, false));
        assert!(set_manual_mark(&mut list, &g, true));
        assert_eq!(list, ["other.chd", "fake trap.chd"]);
    }

    #[test]
    fn the_key_survives_a_move_of_the_file() {
        let list = vec!["fake trap.chd".to_owned()];
        let moved = game(
            "megacd",
            "/elsewhere/roms/Fake Trap.chd",
            "Fake Trap",
            Identification::Name,
        );
        assert_eq!(cd32x_mark(&moved, &list), Cd32xMark::Manual);
    }

    #[test]
    fn same_name_games_are_counted_among_megacd_only() {
        let a = game("megacd", "/r/a/disc1.cue", "A", Identification::Unknown);
        let b = game("megacd", "/r/b/DISC1.cue", "B", Identification::Unknown);
        let c = game("megacd", "/r/c/other.cue", "C", Identification::Unknown);
        let d = game("saturn", "/r/d/disc1.cue", "D", Identification::Unknown);
        let games = [a.clone(), b, c, d];
        assert_eq!(same_key_count(&games, &a), 2);
    }

    #[test]
    fn invitation_only_when_the_content_did_not_identify_the_game() {
        let none: Vec<String> = Vec::new();
        let by = |id| game("megacd", "/r/x.chd", "x", id);
        assert!(invites_marking(&by(Identification::Name), &none));
        assert!(invites_marking(&by(Identification::Unknown), &none));
        assert!(!invites_marking(&by(Identification::Crc32Size), &none));
        assert!(!invites_marking(&by(Identification::Sha1), &none));
        assert!(!invites_marking(&by(Identification::Serial), &none));
        assert!(!invites_marking(
            &by(Identification::Name),
            &["x.chd".to_owned()]
        ));
        let tagged = game("megacd", "/r/x (32X CD).chd", "x", Identification::Name);
        assert!(!invites_marking(&tagged, &none));
        let md = game("megadrive", "/r/x.md", "x", Identification::Unknown);
        assert!(!invites_marking(&md, &none));
    }
}
