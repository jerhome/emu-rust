use std::cmp::Ordering;

use crate::Game;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Title,
    /// Most recent first; never played last, then by title.
    LastPlayed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameFilter {
    pub system: Option<String>,
    /// Matched against `normalize_title` of the title and of the file name; every word must appear.
    pub search: String,
    pub favorites_only: bool,
    pub sort: SortOrder,
}

fn fold(c: char, out: &mut String) {
    let folded = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'ç' | 'ć' | 'č' => "c",
        'ď' | 'đ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
        'ğ' => "g",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
        'ł' => "l",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
        'ř' => "r",
        'ś' | 'š' | 'ş' => "s",
        'ť' | 'ţ' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => "u",
        'ý' | 'ÿ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        'æ' => "ae",
        'œ' => "oe",
        'ß' => "ss",
        _ if c.is_alphanumeric() => {
            out.push(c);
            return;
        }
        _ => " ",
    };
    out.push_str(folded);
}

/// Lower case, common Latin diacritics removed, punctuation folded to spaces.
pub fn normalize_title(title: &str) -> String {
    let mut folded = String::with_capacity(title.len());
    for c in title.chars().flat_map(char::to_lowercase) {
        fold(c, &mut folded);
    }
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Indices into `games` matching `filter`, in display order. Pure: the UI keeps all games in memory.
pub fn filter_games(games: &[Game], filter: &GameFilter) -> Vec<usize> {
    let search = normalize_title(&filter.search);
    let words: Vec<&str> = search.split(' ').filter(|w| !w.is_empty()).collect();
    let mut keyed: Vec<(usize, String)> = games
        .iter()
        .enumerate()
        .filter(|(_, g)| filter.system.as_ref().is_none_or(|s| *s == g.system))
        .filter(|(_, g)| !filter.favorites_only || g.favorite)
        .filter_map(|(i, g)| {
            let title = normalize_title(&g.title);
            if !words.is_empty() {
                let file = g
                    .path
                    .file_name()
                    .map(|n| normalize_title(&n.to_string_lossy()))
                    .unwrap_or_default();
                if !words.iter().all(|w| title.contains(w) || file.contains(w)) {
                    return None;
                }
            }
            Some((i, title))
        })
        .collect();
    let by_title = |a: &(usize, String), b: &(usize, String)| {
        a.1.cmp(&b.1)
            .then_with(|| games[a.0].title.cmp(&games[b.0].title))
            .then_with(|| games[a.0].id.cmp(&games[b.0].id))
    };
    match filter.sort {
        SortOrder::Title => keyed.sort_by(by_title),
        SortOrder::LastPlayed => keyed.sort_by(|a, b| {
            match (games[a.0].last_played_unix, games[b.0].last_played_unix) {
                (Some(x), Some(y)) => y.cmp(&x),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
            .then_with(|| by_title(a, b))
        }),
    }
    keyed.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GameId, Identification};
    use std::path::PathBuf;

    fn game(id: i64, system: &str, title: &str, file: &str) -> Game {
        Game {
            id: GameId(id),
            system: system.into(),
            path: PathBuf::from("/roms").join(file),
            entry: None,
            title: title.into(),
            identification: Identification::Unknown,
            size: 1,
            crc32: None,
            sha1: None,
            favorite: false,
            last_played_unix: None,
            play_count: 0,
            media: Vec::new(),
        }
    }

    #[test]
    fn normalization() {
        assert_eq!(
            normalize_title("  Pokémon - Édition Rouge (France)!! "),
            "pokemon edition rouge france"
        );
        assert_eq!(normalize_title("Æon: Straße"), "aeon strasse");
        assert_eq!(normalize_title("ÉLITE"), "elite");
        assert_eq!(
            normalize_title("Sonic_the-Hedgehog 2"),
            "sonic the hedgehog 2"
        );
        assert_eq!(normalize_title("!!!"), "");
        assert_eq!(normalize_title("ゼルダ"), "ゼルダ");
    }

    #[test]
    fn filters_and_sorts() {
        let mut games = vec![
            game(1, "gb", "Zelda (Europe)", "zelda.gb"),
            game(2, "snes", "Énigme", "enigma.sfc"),
            game(3, "gb", "alpha", "secret-name.gb"),
            game(4, "nes", "Beta", "beta.nes"),
        ];
        games[1].favorite = true;
        games[3].last_played_unix = Some(10);
        games[0].last_played_unix = Some(20);

        let all = filter_games(&games, &GameFilter::default());
        assert_eq!(all, [2, 3, 1, 0]);
        let gb = GameFilter {
            system: Some("gb".into()),
            ..Default::default()
        };
        assert_eq!(filter_games(&games, &gb), [2, 0]);
        let fav = GameFilter {
            favorites_only: true,
            ..Default::default()
        };
        assert_eq!(filter_games(&games, &fav), [1]);
        let search = |s: &str| GameFilter {
            search: s.into(),
            ..Default::default()
        };
        assert_eq!(filter_games(&games, &search("enig")), [1]);
        assert_eq!(filter_games(&games, &search("ENIGMA")), [1]);
        assert_eq!(filter_games(&games, &search("secret")), [2]);
        assert_eq!(filter_games(&games, &search("zelda europe")), [0]);
        assert_eq!(
            filter_games(&games, &search("zelda japan")),
            Vec::<usize>::new()
        );
        assert_eq!(filter_games(&games, &search(" - ")).len(), 4);
        let recent = GameFilter {
            sort: SortOrder::LastPlayed,
            ..Default::default()
        };
        assert_eq!(filter_games(&games, &recent), [0, 3, 2, 1]);
    }
}
