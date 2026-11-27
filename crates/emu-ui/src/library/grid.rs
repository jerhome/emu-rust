//! Pure helpers of the home screen: grid navigation, system counts, DAT system detection, launch extension.

use std::path::Path;

use emu_library::{Game, SystemInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Nav {
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

/// New selection after `nav` in a grid of `count` cards laid out in `columns` columns; `page` rows per page.
/// Left / Right wrap to the previous / next row; Up / Down stay in the column and stop at the edges.
pub(crate) fn step(selected: usize, count: usize, columns: usize, page: usize, nav: Nav) -> usize {
    if count == 0 {
        return 0;
    }
    let columns = columns.max(1);
    let last = count - 1;
    let selected = selected.min(last);
    let rows = page.max(1) * columns;
    match nav {
        Nav::Left => selected.saturating_sub(1),
        Nav::Right => (selected + 1).min(last),
        Nav::Up => selected.checked_sub(columns).unwrap_or(selected),
        Nav::Down => {
            let next = selected + columns;
            if next <= last {
                next
            } else if last / columns > selected / columns {
                // Partial last row below: land on its last card.
                last
            } else {
                selected
            }
        }
        Nav::PageUp => selected.saturating_sub(rows),
        Nav::PageDown => (selected + rows).min(last),
        Nav::Home => 0,
        Nav::End => last,
    }
}

/// Columns that fit in `width` for cards of `card` width separated by `gap`.
pub(crate) fn columns_for(width: f32, card: f32, gap: f32) -> usize {
    if width <= card {
        return 1;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // positive, small
    let n = ((width + gap) / (card + gap)).floor() as usize;
    n.max(1)
}

/// `(system id, game count)` for every catalog system with at least one game, in catalog order.
pub(crate) fn system_counts(games: &[Game], catalog: &[SystemInfo]) -> Vec<(String, usize)> {
    catalog
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                games.iter().filter(|g| g.system == s.id).count(),
            )
        })
        .filter(|(_, n)| *n > 0)
        .collect()
}

/// Short badges of a game, shown on its card and in the status bar of the selected game. `manual`: the manual
/// CD 32X marks (`[library] cd32x`).
pub(crate) fn badges(game: &Game, manual: &[String]) -> Vec<&'static str> {
    let mut out = Vec::new();
    if super::cd32x::effective_needs_32x(game, manual) {
        out.push(super::text::BADGE_32X);
    }
    out
}

/// Extension used to choose the core: the zip entry's for an archive, else the file's (lower case, no dot).
pub(crate) fn launch_extension(game: &Game) -> Option<String> {
    // A folder game's name may contain a dot (`Doom v1.9`): it has no extension.
    if game.is_dir {
        return None;
    }
    let name = game.entry.as_deref().map_or(game.path.as_path(), Path::new);
    name.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// System of a DAT from its header name, then its file stem: exact libretro database name, or that name
/// followed by ` (`/` -` (No-Intro suffixes). The longest name wins (`Game Boy Color` over `Game Boy`).
pub(crate) fn detect_dat_system<'a>(
    dat_name: &str,
    file_stem: &str,
    catalog: &'a [SystemInfo],
) -> Option<&'a SystemInfo> {
    let matches = |name: &str, db: &str| {
        name == db
            || name
                .strip_prefix(db)
                .is_some_and(|rest| rest.starts_with(" (") || rest.starts_with(" -"))
    };
    [dat_name, file_stem].into_iter().find_map(|name| {
        catalog
            .iter()
            .filter_map(|s| Some((s, s.libretro_name()?)))
            .filter(|(_, db)| matches(name.trim(), db))
            .max_by_key(|(_, db)| db.len())
            .map(|(s, _)| s)
    })
}
