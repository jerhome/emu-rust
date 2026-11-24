// The mock core only builds on Unix.
#![cfg(unix)]
//! 4.0 integration of the UI layer (design v4.0 §2, §6) on the mock core named like dosbox_pure: a folder game from
//! the command line (`JEU/START.EXE`), its snapshot built before the launch, the VFS given to the core, the folder
//! never written; the pad sticks reaching an `analog = true` core. The `MOCK_*` protocol is switched on in a child
//! process (the environment of this process is never changed: no `unsafe` here).

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use emu_runtime::{CollectSink, ContentSpec, Emulator, EmulatorOptions, ScriptedInput};
use emu_ui::assets::cores::{CoreId, CorePreferences};
use emu_ui::{
    Config, DataDirs, GameContent, GamepadInput, GamepadSettings, PadAxis, PadEvent, PadEventKind,
    PadId, PadMap, PadStyle, ScriptedPadSource, clear_dos_cache, dos_cli_content,
    prepare_dos_snapshot, resolve_game,
};
use mock_core::Variant;

const CHILD: &str = "EMU_UI_TEST_DOS_CHILD";

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    dirs: DataDirs,
    /// The mock core under the file name of dosbox_pure (`core_id_from_path` → `dosbox_pure`).
    core: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let dirs = DataDirs::under(&root.join("data"));
        dirs.create_all().unwrap();
        let core = root.join("cores").join(format!(
            "dosbox_pure_libretro.{}",
            if cfg!(target_os = "macos") {
                "dylib"
            } else {
                "so"
            }
        ));
        fs::create_dir_all(core.parent().unwrap()).unwrap();
        fs::copy(mock_core::build(Variant::Default), &core).unwrap();
        Self {
            _tmp: tmp,
            root,
            dirs,
            core,
        }
    }

    /// `roms/dos/Jeu/` with two programs and a sub-folder.
    fn folder_game(&self) -> PathBuf {
        let dir = self.root.join("roms/dos/Jeu");
        fs::create_dir_all(dir.join("DATA")).unwrap();
        fs::write(dir.join("START.EXE"), b"MZ start").unwrap();
        fs::write(dir.join("SETUP.EXE"), b"MZ setup").unwrap();
        fs::write(dir.join("DATA/LEVEL1.DAT"), vec![7u8; 10_000]).unwrap();
        dir
    }

    fn options(&self, provide_vfs: bool) -> EmulatorOptions {
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
            port_devices: Vec::new(),
            cache_dir: self.dirs.cache_dir(),
            provide_vfs,
        }
    }
}

fn run_child(test: &str, vars: &[(&str, &str)]) {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", test, "--test-threads=1"])
        .env(CHILD, "1");
    for (k, v) in vars {
        cmd.env(k, v);
    }
    assert!(cmd.status().unwrap().success(), "child test failed");
}

fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(tree(&p));
        } else {
            out.push((p.clone(), fs::read(&p).unwrap()));
        }
    }
    out.sort();
    out
}

/// `--core dosbox_pure --rom Jeu/START.EXE`: snapshot built once (with progress), the core loads
/// `<cache>/dosbox_pure/Jeu.zip#START.EXE`, gets the VFS (`vfs = true`), and the folder is left untouched.
#[test]
fn folder_game_from_the_command_line() {
    if std::env::var_os(CHILD).is_none() {
        return run_child("folder_game_from_the_command_line", &[("MOCK_VFS", "1")]);
    }
    let env = Env::new();
    let game = env.folder_game();
    let before = tree(&game);
    let config = Config::default();
    let content = dos_cli_content(&game.join("start.exe"), &env.core, &config)
        .expect("a folder game")
        .expect("valid program");
    assert_eq!(
        content,
        GameContent::Folder {
            dir: game.clone(),
            start: Some("START.EXE".into())
        }
    );
    let mut last = (0, 0);
    let built = prepare_dos_snapshot(&env.core, &content, &env.dirs, &config, &mut |d, t| {
        last = (d, t)
    })
    .unwrap();
    assert!(built);
    assert_eq!(last.0, last.1);
    assert!(last.1 >= 10_000);
    assert!(
        !prepare_dos_snapshot(&env.core, &content, &env.dirs, &config, &mut |_, _| {}).unwrap(),
        "current snapshot reused"
    );

    let prefs = CorePreferences::builtin();
    let id = CoreId::new("dosbox_pure").unwrap();
    assert!(prefs.provides_vfs(&id));
    let spec = emu_ui::content_spec("dosbox_pure", &content, &prefs);
    assert!(matches!(&spec, ContentSpec::Folder { start: Some(s), .. } if s == "START.EXE"));
    // The mock core opens its path as a file: `Jeu.zip#START.EXE` is for dosbox_pure (runtime tests); the same
    // folder without a program is the same game and the same snapshot.
    let spec = emu_ui::content_spec(
        "dosbox_pure",
        &GameContent::Folder {
            dir: game.clone(),
            start: None,
        },
        &prefs,
    );
    let resolved = resolve_game("dosbox_pure", spec, &env.dirs);
    assert_eq!(resolved.identity.save_name, "Jeu");
    let mut emu = Emulator::load_content_as(
        &env.core,
        resolved.spec,
        resolved.identity,
        env.options(prefs.provides_vfs(&id)),
    )
    .expect("load");
    assert_eq!(emu.content_name(), Some("Jeu"));
    let mut input = ScriptedInput::new();
    for _ in 0..3 {
        emu.run_frame(&mut input, &mut CollectSink::default())
            .unwrap();
    }
    let vfs = fs::read_to_string(emu.identity().core_save_dir.join("mock-vfs.txt")).unwrap();
    assert!(vfs.starts_with("version=3"), "{vfs}");
    drop(emu);
    assert!(env.dirs.cache_dir().join("dosbox_pure/Jeu.zip").is_file());
    assert_eq!(tree(&game), before, "the user's folder is never written");
    assert!(clear_dos_cache(&env.dirs, &config).unwrap() >= 10_000);
    assert!(!env.dirs.cache_dir().join("dosbox_pure/Jeu.zip").exists());
}

/// `.conf` is refused for dosbox_pure, a folder for a core that does not read folders.
#[test]
fn refused_contents() {
    let env = Env::new();
    let config = Config::default();
    let conf = env.root.join("dosbox.conf");
    fs::write(&conf, b"[autoexec]").unwrap();
    assert!(dos_cli_content(&conf, &env.core, &config).unwrap().is_err());
    let game = env.folder_game();
    assert!(
        dos_cli_content(&game, Path::new("/c/hatari_libretro.so"), &config)
            .unwrap()
            .is_err()
    );
    assert!(dos_cli_content(&env.root.join("x.zip"), &env.core, &config).is_none());
}

fn axis(axis: PadAxis, value: f32) -> PadEvent {
    PadEvent {
        id: PadId(1),
        kind: PadEventKind::Axis { axis, value },
    }
}

/// DG1: the pad sticks reach the core as `RETRO_DEVICE_ANALOG` for an `analog = true` core.
#[test]
fn pad_sticks_reach_an_analog_core() {
    if std::env::var_os(CHILD).is_none() {
        return run_child("pad_sticks_reach_an_analog_core", &[("MOCK_ANALOG", "0")]);
    }
    let env = Env::new();
    let game = env.folder_game();
    let prefs = CorePreferences::builtin();
    assert!(prefs.analog(&CoreId::new("dosbox_pure").unwrap()));
    let content = GameContent::Folder {
        dir: game,
        start: None,
    };
    let spec = emu_ui::content_spec("dosbox_pure", &content, &prefs);
    let resolved = resolve_game("dosbox_pure", spec, &env.dirs);
    let mut emu = Emulator::load_content_as(
        &env.core,
        resolved.spec,
        resolved.identity,
        env.options(false),
    )
    .unwrap();
    let mut source = ScriptedPadSource::new();
    source.push_batch(vec![
        PadEvent {
            id: PadId(1),
            kind: PadEventKind::Connected {
                name: "Wireless Controller".into(),
                guid: "030000004c050000c405000000000000".into(),
                raw: false,
            },
        },
        axis(PadAxis::LeftX, 1.0),
        axis(PadAxis::RightY, 1.0),
    ]);
    let mut pad = GamepadInput::new(
        source,
        PadMap::default_for(PadStyle::Generic),
        GamepadSettings::default(),
    );
    pad.set_analog(true);
    emu.run_frame(&mut pad, &mut CollectSink::default())
        .unwrap();
    let text = fs::read_to_string(emu.identity().core_save_dir.join("mock-analog.txt")).unwrap();
    assert!(text.contains("left=32767,0"), "{text}");
    assert!(text.contains("right=0,-32767"), "{text}");
}
