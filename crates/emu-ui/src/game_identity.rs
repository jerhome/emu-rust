//! Game identity on the UI side (design `v2.1-identite-disques.md` §3.2): resolved before loading, because the
//! per-game options file is named after it, then handed to `Emulator::load_content_as`.

use std::path::{Path, PathBuf};

use emu_runtime::{
    ContentSpec, GameIdentity, StateOrigin, identity_request_for, resolve_game_identity,
};

use crate::DataDirs;
use crate::content::game_key;
use crate::game_options::{game_options_path, inherit_game_options};

/// What `App::build` needs before loading a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGame {
    pub spec: ContentSpec,
    pub identity: GameIdentity,
    /// `<data>/game-options/<core_id>/<save_name>.toml`.
    pub options_path: PathBuf,
    /// `<base>.toml` was copied to `<save_name>.toml` (homonym of a game played under the shared 2.0 name).
    pub options_inherited: bool,
    /// The registry could not be used at all (I/O error): 2.0 name for this session.
    pub resolution_failed: bool,
}

/// Resolves the identity of `spec` for core `core_id` (registry of `<saves>/<core_id>/`), then the options path;
/// copies the shared 2.0 options to a homonym that inherits them. Never fails: on error the 2.0 name is used.
pub fn resolve_game(core_id: &str, spec: ContentSpec, dirs: &DataDirs) -> ResolvedGame {
    let saves_core_dir = dirs.saves.join(core_id);
    let states_core_dir = dirs.states.join(core_id);
    let request = identity_request_for(&spec, &saves_core_dir, &states_core_dir);
    let (identity, resolution_failed) = match resolve_game_identity(&request) {
        Ok(identity) => (identity, false),
        Err(e) => {
            tracing::warn!("identité du jeu non résolue ({e}) : nom 2.0 utilisé");
            (
                GameIdentity::legacy(&request.base_name, &saves_core_dir),
                true,
            )
        }
    };
    let options_inherited = identity.inherits_legacy()
        && match inherit_game_options(
            &dirs.game_options,
            core_id,
            identity.base_name(),
            game_key(&identity),
        ) {
            Ok(copied) => copied,
            Err(e) => {
                tracing::warn!("options du jeu non reprises : {e}");
                false
            }
        };
    let options_path = game_options_path(&dirs.game_options, core_id, game_key(&identity));
    ResolvedGame {
        spec,
        identity,
        options_path,
        options_inherited,
        resolution_failed,
    }
}

/// Loading a state that may belong to another file of the same name asks first (default « Non »); states of
/// 1.0 / 2.0 without a homonym (`Unknown`) and the game's own states load at once.
pub fn state_load_needs_confirmation(origin: Option<StateOrigin>) -> bool {
    matches!(
        origin,
        Some(StateOrigin::OtherContent | StateOrigin::Ambiguous)
    )
}

/// Working-copy folder of the session (same rule as the runtime's): `None` for a `Direct` set.
pub(crate) fn session_work_dir(
    saves_core_dir: &Path,
    direct: bool,
    prepared_work_dir: Option<Option<&Path>>,
    save_name: &str,
) -> Option<PathBuf> {
    if direct {
        return None;
    }
    match prepared_work_dir {
        Some(dir) => dir.map(Path::to_path_buf),
        None => Some(saves_core_dir.join(format!("{save_name}.disks"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_only_for_other_content_and_ambiguous() {
        assert!(state_load_needs_confirmation(Some(
            StateOrigin::OtherContent
        )));
        assert!(state_load_needs_confirmation(Some(StateOrigin::Ambiguous)));
        assert!(!state_load_needs_confirmation(Some(StateOrigin::Same)));
        assert!(!state_load_needs_confirmation(Some(StateOrigin::Unknown)));
        assert!(!state_load_needs_confirmation(None));
    }

    #[test]
    fn work_dir_follows_the_runtime_rule() {
        let saves = Path::new("/s/hatari");
        assert_eq!(session_work_dir(saves, true, None, "G"), None);
        assert_eq!(
            session_work_dir(saves, false, None, "no-game"),
            Some(PathBuf::from("/s/hatari/no-game.disks"))
        );
        assert_eq!(
            session_work_dir(
                saves,
                false,
                Some(Some(Path::new("/s/hatari/G (2).disks"))),
                "G"
            ),
            Some(PathBuf::from("/s/hatari/G (2).disks"))
        );
        assert_eq!(session_work_dir(saves, false, Some(None), "G"), None);
    }

    #[test]
    fn homonyms_get_separate_options_and_inherit_the_shared_ones() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = DataDirs::under(&dir.path().join("data"));
        let roms = dir.path().join("roms");
        for sub in ["a", "b"] {
            std::fs::create_dir_all(roms.join(sub)).unwrap();
        }
        let a = roms.join("a/Game.sfc");
        let b = roms.join("b/Game.sfc");
        std::fs::write(&a, b"rom a").unwrap();
        std::fs::write(&b, b"rom b").unwrap();
        // 2.0 files of the shared name: the first game's entry is `legacy`.
        let saves = dirs.saves.join("snes9x");
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(saves.join("Game.srm"), b"sram").unwrap();
        let shared = game_options_path(&dirs.game_options, "snes9x", "Game");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, "[media]\nlast_disk = 1\n").unwrap();

        let first = resolve_game("snes9x", ContentSpec::File(a.clone()), &dirs);
        assert_eq!(first.identity.save_name, "Game");
        assert_eq!(first.options_path, shared);
        assert!(!first.options_inherited && !first.resolution_failed);

        let second = resolve_game("snes9x", ContentSpec::File(b), &dirs);
        assert_eq!(second.identity.save_name, "Game (2)");
        assert!(second.options_inherited);
        assert_eq!(
            std::fs::read(&second.options_path).unwrap(),
            std::fs::read(&shared).unwrap()
        );
        // Idempotent: the next launches keep their names and copy nothing.
        let again = resolve_game("snes9x", ContentSpec::File(a), &dirs);
        assert_eq!(again.identity.save_name, "Game");
        assert!(!again.options_inherited);
    }
}
