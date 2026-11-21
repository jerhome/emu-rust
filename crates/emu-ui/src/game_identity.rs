//! Game identity on the UI side (design `v2.1-identite-disques.md` §3.2): resolved before loading, because the
//! per-game options file is named after it, then handed to `Emulator::load_content_as`.

use std::path::PathBuf;

use emu_runtime::{
    ContentSpec, GameIdentity, SaveScope, StateOrigin, identity_request_for, resolve_game_identity,
};

use emu_assets::cores::CorePreferences;

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
/// The `cores.toml` `save_scope` of the core applies (`Game` ⇒ `core_save_dir` is the game's own folder, design
/// v5.0 §10 n° 1), the fallback identity included.
pub fn resolve_game(core_id: &str, spec: ContentSpec, dirs: &DataDirs) -> ResolvedGame {
    let scope = crate::content::save_scope(&CorePreferences::builtin(), core_id);
    resolve_game_with(core_id, spec, dirs, scope)
}

/// [`resolve_game`] with an explicit save scope.
pub(crate) fn resolve_game_with(
    core_id: &str,
    spec: ContentSpec,
    dirs: &DataDirs,
    scope: SaveScope,
) -> ResolvedGame {
    let saves_core_dir = dirs.saves.join(core_id);
    let states_core_dir = dirs.states.join(core_id);
    let mut request = identity_request_for(&spec, &saves_core_dir, &states_core_dir);
    request.scope = scope;
    let (identity, resolution_failed) = match resolve_game_identity(&request) {
        Ok(identity) => (identity, false),
        Err(e) => {
            tracing::warn!("identité du jeu non résolue ({e}) : nom 2.0 utilisé");
            (request.fallback_identity(), true)
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

    /// `save_scope = "game"` (cap32): every game gets its own folder, the first one included.
    #[test]
    fn game_scope_gives_every_game_its_own_core_folder() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = DataDirs::under(&dir.path().join("data"));
        let rom = dir.path().join("Sorcery.dsk");
        std::fs::write(&rom, b"disk").unwrap();
        let r = resolve_game("cap32", ContentSpec::File(rom.clone()), &dirs);
        assert_eq!(r.identity.save_name, "Sorcery");
        assert_eq!(
            r.identity.core_save_dir,
            dirs.saves.join("cap32").join("Sorcery")
        );
        let flat = resolve_game_with("cap32", ContentSpec::File(rom), &dirs, SaveScope::Core);
        assert_eq!(flat.identity.core_save_dir, dirs.saves.join("cap32"));
    }
}
