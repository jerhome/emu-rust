//! `Library::relocate` (ADR 0023 §4.5 step 3): rewrites the paths of moved games in one transaction. The rows are
//! kept, so favourite, history, identification, region and covers follow. Keys are built like the scan's.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use rusqlite::{Connection, params};

use crate::LibraryError;
use crate::db::db_error;
use crate::scan::{folders_of, normalize_root};

/// Temporary value of a path being rewritten: `game.path` and `game_media.path` are UNIQUE, and a batch may move a
/// row onto the old place of another one. NUL never occurs in a file path.
const PENDING: &str = "\u{0}tidy\u{0}";

fn invalid(path: &Path, message: &str) -> LibraryError {
    LibraryError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, message.to_owned()),
    }
}

fn plain_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// New key of `path` under `moves` (`(from, to)` absolute keys): the move of the path itself, or of a folder
/// holding it. First match wins.
fn moved_to(path: &str, moves: &[(PathBuf, PathBuf)]) -> Option<(String, usize)> {
    let p = Path::new(path);
    moves.iter().enumerate().find_map(|(i, (from, to))| {
        let rest = p.strip_prefix(from).ok()?;
        let new = if rest.as_os_str().is_empty() {
            to.clone()
        } else {
            to.join(rest)
        };
        Some((new.to_str()?.to_owned(), i))
    })
}

pub(super) fn run(
    conn: &mut Connection,
    roms_root: &Path,
    moves: &[(PathBuf, PathBuf)],
) -> Result<u64, LibraryError> {
    let root = normalize_root(roms_root);
    let base = root.parent().unwrap_or(&root).to_path_buf();
    let mut abs = Vec::with_capacity(moves.len());
    for (from, to) in moves {
        for p in [from, to] {
            if !plain_relative(p) {
                return Err(invalid(
                    p,
                    "chemin de rangement invalide (relatif sans « .. » attendu)",
                ));
            }
            if root.join(p).to_str().is_none() {
                return Err(invalid(p, "chemin non UTF-8"));
            }
        }
        abs.push((root.join(from), root.join(to)));
    }
    if abs.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction().map_err(db_error)?;
    let games: Vec<(i64, String, String)> = {
        let mut stmt = tx
            .prepare("SELECT id, path, system FROM game")
            .map_err(db_error)?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?
    };
    let media: Vec<(i64, i64, String)> = {
        let mut stmt = tx
            .prepare("SELECT game_id, idx, path FROM game_media")
            .map_err(db_error)?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?
    };
    // (id, new path, system, index of the move)
    let game_moves: Vec<(i64, String, &str, usize)> = games
        .iter()
        .filter_map(|(id, path, system)| {
            moved_to(path, &abs).map(|(new, i)| (*id, new, system.as_str(), i))
        })
        .collect();
    let media_moves: Vec<(i64, i64, String, usize)> = media
        .iter()
        .filter_map(|(game, idx, path)| moved_to(path, &abs).map(|(new, i)| (*game, *idx, new, i)))
        .collect();
    let moved_games: HashSet<i64> = game_moves.iter().map(|m| m.0).collect();
    let moved_media: HashSet<(i64, i64)> = media_moves.iter().map(|m| (m.0, m.1)).collect();

    // A row already at a destination is stale when its file is absent or when the source of the move is gone (the
    // file was moved there: that row duplicates it); both present means the call does not match the disk.
    let stale = |new: &str, move_index: usize| -> Result<(), LibraryError> {
        let (from, _) = &abs[move_index];
        if exists(Path::new(new)) && exists(from) {
            return Err(LibraryError::Io {
                path: PathBuf::from(new),
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "un autre jeu de la bibliothèque occupe déjà cet emplacement",
                ),
            });
        }
        Ok(())
    };
    let game_targets: HashMap<&str, usize> =
        game_moves.iter().map(|m| (m.1.as_str(), m.3)).collect();
    for (id, path, _) in &games {
        if moved_games.contains(id) {
            continue;
        }
        if let Some(&i) = game_targets.get(path.as_str()) {
            stale(path, i)?;
            tx.execute("DELETE FROM game WHERE id = ?1", [id])
                .map_err(db_error)?;
        }
    }
    let media_targets: HashMap<&str, usize> =
        media_moves.iter().map(|m| (m.2.as_str(), m.3)).collect();
    for (game, idx, path) in &media {
        if moved_media.contains(&(*game, *idx)) {
            continue;
        }
        if let Some(&i) = media_targets.get(path.as_str()) {
            stale(path, i)?;
            tx.execute(
                "DELETE FROM game_media WHERE game_id = ?1 AND idx = ?2",
                params![game, idx],
            )
            .map_err(db_error)?;
        }
    }

    for (id, _, _, _) in &game_moves {
        tx.execute(
            "UPDATE game SET path = ?2 || id WHERE id = ?1",
            params![id, PENDING],
        )
        .map_err(db_error)?;
    }
    for (game, idx, _, _) in &media_moves {
        tx.execute(
            "UPDATE game_media SET path = ?3 || game_id || ':' || idx WHERE game_id = ?1 AND idx = ?2",
            params![game, idx, PENDING],
        )
        .map_err(db_error)?;
    }
    for (id, new, system, _) in &game_moves {
        // The folder rule of the scan at the new place: the next scan then finds the row unchanged.
        let folders = folders_of(&base, Path::new(new).parent().unwrap_or(Path::new("")));
        let pinned = folders.iter().any(|f| f == system);
        tx.execute(
            "UPDATE game SET path = ?2, pinned = ?3 WHERE id = ?1",
            params![id, new, pinned],
        )
        .map_err(db_error)?;
    }
    for (game, idx, new, _) in &media_moves {
        tx.execute(
            "UPDATE game_media SET path = ?3 WHERE game_id = ?1 AND idx = ?2",
            params![game, idx, new],
        )
        .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)?;
    Ok(game_moves.len() as u64)
}
