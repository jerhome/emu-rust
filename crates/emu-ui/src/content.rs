//! From the files of a game to the `ContentSpec` the runtime loads (ADR 0015 §5). Pure, tested.

use std::path::{Path, PathBuf};

use emu_assets::cores::{CoreId, CoreMediaPolicy, CorePreferences};
use emu_runtime::{ContentSpec, MediaPolicy, MediaSet};

/// What to boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameContent {
    /// One file (1.0), the disks of one game in order, or one `.m3u`.
    Files(Vec<PathBuf>),
    /// No content (`SET_SUPPORT_NO_GAME`): the computer's desktop.
    NoGame,
}

impl GameContent {
    /// First file, if any (pad style detection, logs).
    pub fn first(&self) -> Option<&Path> {
        match self {
            Self::Files(f) => f.first().map(PathBuf::as_path),
            Self::NoGame => None,
        }
    }
}

fn is_m3u(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u"))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Name of a disk set: the library group name (without `(Disk N of M)`), else the first file's stem.
pub fn media_set_name(files: &[PathBuf]) -> String {
    let (groups, _) = emu_library::group_media(files);
    groups
        .into_iter()
        .find(|g| g.members.first() == files.first())
        .map(|g| g.name)
        .or_else(|| files.first().map(|f| stem(f)))
        .unwrap_or_default()
}

/// `File` for a single file of a `direct` core (1.0 behaviour, zips included); `Media` for several disks, a
/// `.m3u`, or any content of a `working_copy` core (hatari writes into its disks: the user's file must never be
/// handed over, even alone); `NoGame` without files.
pub fn content_spec(core_id: &str, content: &GameContent, prefs: &CorePreferences) -> ContentSpec {
    let files = match content {
        GameContent::NoGame => return ContentSpec::NoGame,
        GameContent::Files(f) if f.is_empty() => return ContentSpec::NoGame,
        GameContent::Files(f) => f,
    };
    let policy = match CoreId::new(core_id).map(|id| prefs.media_policy(&id)) {
        Some(CoreMediaPolicy::WorkingCopy) => MediaPolicy::WorkingCopy,
        _ => MediaPolicy::Direct,
    };
    if files.len() == 1 && is_m3u(&files[0]) {
        return ContentSpec::Media {
            set: MediaSet {
                name: stem(&files[0]),
                images: Vec::new(),
                playlist: Some(files[0].clone()),
            },
            policy,
        };
    }
    if files.len() == 1 && policy == MediaPolicy::Direct {
        return ContentSpec::File(files[0].clone());
    }
    ContentSpec::Media {
        set: MediaSet {
            name: media_set_name(files),
            images: files.clone(),
            playlist: None,
        },
        policy,
    }
}

/// Key of the game's options file and working copies, known before loading (same rules as the runtime).
pub(crate) fn game_key(spec: &ContentSpec) -> String {
    match spec {
        ContentSpec::File(p) => emu_runtime::game_name_from_path(p),
        ContentSpec::Media { set, .. } => set.name.clone(),
        _ => emu_runtime::NO_GAME_NAME.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> GameContent {
        GameContent::Files(names.iter().map(PathBuf::from).collect())
    }

    #[test]
    fn hatari_always_gets_working_copies() {
        let prefs = CorePreferences::builtin();
        match content_spec("hatari", &files(&["/g/Nova.st"]), &prefs) {
            ContentSpec::Media { set, policy } => {
                assert_eq!(policy, MediaPolicy::WorkingCopy);
                assert_eq!(set.name, "Nova");
                assert_eq!(set.images, vec![PathBuf::from("/g/Nova.st")]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn consoles_keep_the_1_0_path() {
        let prefs = CorePreferences::builtin();
        assert_eq!(
            content_spec("snes9x", &files(&["/g/a.sfc"]), &prefs),
            ContentSpec::File(PathBuf::from("/g/a.sfc"))
        );
        assert_eq!(
            content_spec("hatari", &GameContent::NoGame, &prefs),
            ContentSpec::NoGame
        );
        assert_eq!(
            content_spec("hatari", &files(&[]), &prefs),
            ContentSpec::NoGame
        );
    }

    #[test]
    fn disk_sets_and_playlists() {
        let prefs = CorePreferences::builtin();
        let set = files(&[
            "/g/Game (1990)(Pub)(Disk 1 of 2).st",
            "/g/Game (1990)(Pub)(Disk 2 of 2).st",
        ]);
        match content_spec("hatarib", &set, &prefs) {
            ContentSpec::Media { set, policy } => {
                assert_eq!(policy, MediaPolicy::Direct);
                assert_eq!(set.name, "Game (1990)(Pub)");
                assert_eq!(set.images.len(), 2);
            }
            other => panic!("{other:?}"),
        }
        match content_spec("hatari", &files(&["/g/Jeu.M3U"]), &prefs) {
            ContentSpec::Media { set, .. } => {
                assert_eq!(set.playlist, Some(PathBuf::from("/g/Jeu.M3U")));
                assert_eq!(set.name, "Jeu");
            }
            other => panic!("{other:?}"),
        }
        // Unrelated files: named after the first one.
        assert_eq!(
            media_set_name(&[PathBuf::from("/a/x.st"), PathBuf::from("/a/y.st")]),
            "x"
        );
    }
}
