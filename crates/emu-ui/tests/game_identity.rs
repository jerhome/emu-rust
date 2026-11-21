// The mock core only builds on Unix.
#![cfg(unix)]
//! 2.1 game identity through the UI path (`content_spec` → `resolve_game` → `Emulator::load_content_as`) on the
//! mock core: two homonyms keep separate SRAM, states, options and core save folders; the confirmation rule of
//! state loads (`state_load_needs_confirmation`) on real state files.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    ContentSpec, Emulator, EmulatorOptions, IdentityNotice, JoypadButton, NullSink, ScriptedInput,
    StateOrigin, core_id_from_path, decode_state, encode_state,
};
use emu_ui::assets::cores::CorePreferences;
use emu_ui::{DataDirs, GameContent, content_spec, resolve_game, state_load_needs_confirmation};
use mock_core::Variant;

/// One libretro core per process (ADR 0008).
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    dirs: DataDirs,
    core: PathBuf,
    core_id: String,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let core = mock_core::build(Variant::Default);
        Self {
            dirs: DataDirs::under(&root.join("data")),
            core_id: core_id_from_path(&core),
            root,
            core,
            _tmp: tmp,
        }
    }

    fn rom(&self, rel: &str, data: &[u8]) -> PathBuf {
        let path = self.root.join("roms").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }

    fn options(&self) -> EmulatorOptions {
        EmulatorOptions {
            system_dir: self.dirs.system.clone(),
            save_dir: self.dirs.saves.clone(),
            state_dir: self.dirs.states.clone(),
            assets_dir: self.dirs.assets.clone(),
            core_variables: Vec::new(),
            restore_added_disks: true,
            core_save_scope: emu_runtime::SaveScope::Core,
            fastforward_override: false,
            variable_state_size: false,
            cache_dir: self.dirs.root.join("cache"),
            provide_vfs: false,
        }
    }

    /// The UI's launch path, as `App::build` does it.
    fn launch(&self, rom: &Path) -> (emu_ui::ResolvedGame, Emulator) {
        let spec = content_spec(
            &self.core_id,
            &GameContent::Files(vec![rom.to_path_buf()]),
            &CorePreferences::builtin(),
        );
        assert_eq!(spec, ContentSpec::File(rom.to_path_buf()));
        let resolved = resolve_game(&self.core_id, spec, &self.dirs);
        let emu = Emulator::load_content_as(
            &self.core,
            resolved.spec.clone(),
            resolved.identity.clone(),
            self.options(),
        )
        .expect("load mock core");
        (resolved, emu)
    }

    fn saves(&self) -> PathBuf {
        self.dirs.saves.join(&self.core_id)
    }

    fn states(&self) -> PathBuf {
        self.dirs.states.join(&self.core_id)
    }
}

/// Holds Start so the mock core writes its frame counter into SRAM, then saves state 0.
fn play(emu: &mut Emulator, frames: u64) {
    let mut input = ScriptedInput::new().press(0, JoypadButton::Start, 0..frames);
    let mut audio = NullSink;
    for _ in 0..frames {
        emu.run_frame(&mut input, &mut audio).unwrap();
    }
    emu.save_state(0).unwrap();
}

#[test]
fn homonyms_are_separated_end_to_end() {
    let _guard = lock();
    let env = Env::new();
    let a = env.rom("a/Game.md", b"first game");
    let b = env.rom("b/Game.md", b"second game, other bytes");

    let (first, mut emu) = env.launch(&a);
    assert_eq!(first.identity.save_name, "Game");
    assert_eq!(first.identity.notice, None);
    play(&mut emu, 20);
    drop(emu);

    let (second, mut emu) = env.launch(&b);
    assert_eq!(second.identity.save_name, "Game (2)");
    match &second.identity.notice {
        Some(IdentityNotice::Homonym {
            first,
            sram_copied,
            legacy_states,
        }) => {
            assert_eq!(fs::canonicalize(first).unwrap(), a);
            assert!(!sram_copied && !legacy_states);
        }
        other => panic!("{other:?}"),
    }
    assert_ne!(first.options_path, second.options_path);
    assert!(second.options_path.ends_with("Game (2).toml"));
    // GET_SAVE_DIRECTORY of the homonym: its own folder (the mock core writes a file there at load).
    assert_eq!(second.identity.core_save_dir, env.saves().join("Game (2)"));
    assert!(env.saves().join("Game (2)/mock-was-here.txt").is_file());
    play(&mut emu, 35);
    drop(emu);

    let sram_a = fs::read(env.saves().join("Game.srm")).unwrap();
    let sram_b = fs::read(env.saves().join("Game (2).srm")).unwrap();
    assert_ne!(sram_a, sram_b, "each homonym keeps its own SRAM");
    assert!(env.states().join("Game.state0").is_file());
    assert!(env.states().join("Game (2).state0").is_file());

    // Next launches: same names, no banner, each finds its own state.
    let (again, emu) = env.launch(&a);
    assert_eq!(again.identity.save_name, "Game");
    assert_eq!(again.identity.notice, None);
    assert_eq!(emu.state_origin(0), Some(StateOrigin::Same));
    assert!(!state_load_needs_confirmation(emu.state_origin(0)));
    drop(emu);
}

#[test]
fn state_loads_ask_only_for_other_content_or_ambiguous_states() {
    let _guard = lock();
    let env = Env::new();
    let a = env.rom("a/Game.md", b"first game");
    let (_, mut emu) = env.launch(&a);
    play(&mut emu, 10);
    let own = fs::read(env.states().join("Game.state0")).unwrap();
    drop(emu);

    // A 2.0 state (no `game_fp`) without any homonym: loads at once.
    let (header, payload) = decode_state(&own).unwrap();
    let legacy = encode_state(
        &emu_runtime::StateHeader {
            game_fp: None,
            ..header.clone()
        },
        payload,
    );
    fs::write(env.states().join("Game.state1"), &legacy).unwrap();
    let (_, emu) = env.launch(&a);
    assert_eq!(emu.state_origin(1), Some(StateOrigin::Unknown));
    assert!(!state_load_needs_confirmation(emu.state_origin(1)));
    assert_eq!(emu.state_origin(5), None, "empty slot");
    drop(emu);

    // Same path, other content (ROM replaced): same identity, its states may not match.
    fs::write(&a, b"patched first game").unwrap();
    let (resolved, emu) = env.launch(&a);
    assert_eq!(resolved.identity.save_name, "Game");
    assert_eq!(emu.state_origin(0), Some(StateOrigin::OtherContent));
    assert!(state_load_needs_confirmation(emu.state_origin(0)));
    drop(emu);

    // Once a homonym exists, the 2.0 state of the shared name is ambiguous.
    let b = env.rom("b/Game.md", b"second game");
    let (second, emu) = env.launch(&b);
    assert_eq!(second.identity.save_name, "Game (2)");
    drop(emu);
    let (_, emu) = env.launch(&a);
    assert_eq!(emu.state_origin(1), Some(StateOrigin::Ambiguous));
    assert!(state_load_needs_confirmation(emu.state_origin(1)));
    drop(emu);
}
