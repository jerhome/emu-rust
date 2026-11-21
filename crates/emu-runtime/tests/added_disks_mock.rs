// The mock core only builds on Unix; on Windows most helpers are unused.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
// `std::env::set_var` is `unsafe` (edition 2024): the mock core reads its protocol with getenv().
#![allow(unsafe_code)]
//! 2.1 disks added from the menu (`docs/design/v2.1-identite-disques.md` §2.3) on the mock core: restored at the
//! next session in the same order, removable, orphans adopted. `MOCK_*` variables are process-global: this
//! binary holds a single test running the scenarios in sequence.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::path::{Path, PathBuf};

use emu_runtime::{
    ContentSpec, Emulator, EmulatorOptions, MediaPolicy, MediaSet, NullInput, NullSink, OrphanKind,
    RuntimeError, core_id_from_path, orphan_copies,
};

fn set_disk(mode: Option<&str>) {
    for name in [
        "MOCK_KEYBOARD",
        "MOCK_MOUSE",
        "MOCK_DISK",
        "MOCK_DISK_DUMP",
        "MOCK_NO_GAME",
    ] {
        // SAFETY: the only test of this binary; no other thread reads or writes the environment meanwhile.
        unsafe { std::env::remove_var(name) };
    }
    if let Some(mode) = mode {
        // SAFETY: as above.
        unsafe { std::env::set_var("MOCK_DISK", mode) };
    }
}

struct Env {
    root: PathBuf,
    lib: PathBuf,
}

impl Env {
    fn new(lib: &Path, name: &str) -> Self {
        let root = mock_core::output_dir().join("emu-runtime-added").join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("roms")).unwrap();
        Self {
            root,
            lib: lib.to_path_buf(),
        }
    }

    fn options(&self, restore: bool) -> EmulatorOptions {
        let data = self.root.join("data");
        EmulatorOptions {
            system_dir: data.join("system"),
            save_dir: data.join("saves"),
            state_dir: data.join("states"),
            assets_dir: data.join("assets"),
            core_variables: Vec::new(),
            restore_added_disks: restore,
            core_save_scope: emu_runtime::SaveScope::Core,
            fastforward_override: false,
            variable_state_size: false,
            cache_dir: data.join("cache"),
            provide_vfs: false,
        }
    }

    fn file(&self, name: &str, data: &[u8]) -> PathBuf {
        let path = self.root.join("roms").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        fs::canonicalize(path).unwrap()
    }

    fn saves(&self) -> PathBuf {
        self.root
            .join("data/saves")
            .join(core_id_from_path(&self.lib))
    }

    fn game(&self, image: &Path) -> ContentSpec {
        ContentSpec::Media {
            set: MediaSet {
                name: "Game".into(),
                images: vec![image.to_path_buf()],
                playlist: None,
                launch: None,
            },
            rules: emu_runtime::MediaRules::new(MediaPolicy::WorkingCopy),
        }
    }

    fn open(&self, spec: ContentSpec, restore: bool) -> Emulator {
        let mut emu = Emulator::load_content(&self.lib, spec, self.options(restore)).unwrap();
        emu.run_frame(&mut NullInput, &mut NullSink).unwrap();
        emu
    }
}

fn paths(emu: &mut Emulator) -> Vec<PathBuf> {
    emu.disks()
        .unwrap()
        .slots()
        .into_iter()
        .map(|s| s.path.unwrap())
        .collect()
}

#[cfg(unix)]
#[test]
fn added_disks_on_mock_core() {
    let lib = mock_core::build(mock_core::Variant::Default);
    restored_in_order(&lib);
    forget_and_adopt(&lib);
    beyond_what_the_core_takes(&lib);
    without_disk_interface(&lib);
    file_session(&lib);
    basic_interface_never_deletes_a_possibly_inserted_copy(&lib);
    set_disk(None);
}

fn restored_in_order(lib: &Path) {
    set_disk(Some("ext"));
    let env = Env::new(lib, "restored");
    let game = env.file("Game/Disk 1.st", b"game");
    let save = env.file("Extras/Save.st", b"blank save");
    let tools = env.file("Extras/Tools.st", b"tools");
    let work = env.saves().join("Game.disks");

    let mut emu = env.open(env.game(&game), true);
    {
        let mut disks = emu.disks().unwrap();
        assert!(disks.added().is_empty());
        assert_eq!(disks.add(&save).unwrap(), 1);
        assert_eq!(disks.add(&tools).unwrap(), 2);
        let added = disks.added();
        assert_eq!(added.len(), 2);
        assert_eq!(
            (added[0].index, added[0].source.as_deref()),
            (Some(1), Some(save.as_path()))
        );
    }
    let first = paths(&mut emu);
    fs::write(&first[1], b"saved game").unwrap();
    drop(emu);

    // Next session: same list, same order, same copies; the last disk index is still valid.
    let mut emu = env.open(env.game(&game), true);
    assert_eq!(paths(&mut emu), first);
    assert_eq!(emu.prepared_media().unwrap().images, first);
    let mut disks = emu.disks().unwrap();
    disks.insert(2).unwrap();
    assert_eq!(disks.current(), Some(2));
    let added = disks.added();
    assert_eq!(
        added.iter().map(|a| a.index).collect::<Vec<_>>(),
        [Some(1), Some(2)]
    );
    assert!(added.iter().all(|a| a.source_present));
    drop(emu);
    assert_eq!(fs::read(&first[1]).unwrap(), b"saved game");

    // The source is gone: the copy (and its progress) comes back anyway.
    fs::remove_file(&save).unwrap();
    let mut emu = env.open(env.game(&game), true);
    assert_eq!(paths(&mut emu), first);
    let added = emu.disks().unwrap().added();
    assert!(!added[0].source_present && added[1].source_present);
    drop(emu);

    // Restoration is opt-in: the list is the set alone, the manifest still remembers the disks.
    let mut emu = env.open(env.game(&game), false);
    assert_eq!(paths(&mut emu), [work.join("Disk 1.st")]);
    let added = emu.disks().unwrap().added();
    assert_eq!(added.len(), 2);
    assert!(added.iter().all(|a| a.index.is_none()));
}

fn forget_and_adopt(lib: &Path) {
    set_disk(Some("ext"));
    let env = Env::new(lib, "forget");
    let game = env.file("Game/Disk 1.st", b"game");
    let a = env.file("Extras/A.st", b"aaaa");
    let b = env.file("Extras/B.st", b"bbbbbb");
    let work = env.saves().join("Game.disks");
    let mut emu = env.open(env.game(&game), true);
    let mut disks = emu.disks().unwrap();
    disks.add(&a).unwrap();
    disks.add(&b).unwrap();
    let (copy_a, copy_b) = (work.join("A.st"), work.join("B.st"));

    disks.insert(1).unwrap();
    assert!(matches!(
        disks.forget_added(&copy_a, false),
        Err(RuntimeError::DiskInserted)
    ));
    assert!(matches!(
        disks.forget_added(&work.join("Disk 1.st"), false),
        Err(RuntimeError::RemovalRefused { .. })
    ));
    disks.insert(2).unwrap();
    // B is inserted at index 2; removing A (index 1) keeps B in the drive, now at index 1.
    disks.forget_added(&copy_a, false).unwrap();
    assert_eq!(disks.current(), Some(1));
    let slots = disks.slots();
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[1].path.as_deref(), Some(copy_b.as_path()));
    assert!(copy_a.is_file(), "copy kept");
    drop(emu);

    // Not restored any more; the kept copy is an orphan that can be taken back.
    let mut emu = env.open(env.game(&game), true);
    assert_eq!(paths(&mut emu), [work.join("Disk 1.st"), copy_b.clone()]);
    let orphans = orphan_copies(&env.saves(), &work).unwrap();
    assert_eq!(orphans.len(), 1);
    assert_eq!(
        (orphans[0].name.as_str(), orphans[0].kind),
        ("A.st", OrphanKind::Copy)
    );
    let mut disks = emu.disks().unwrap();
    assert!(disks.adopt_orphan("B.st").is_err(), "referenced");
    assert!(disks.adopt_orphan("../A.st").is_err());
    assert_eq!(disks.adopt_orphan("A.st").unwrap(), 2);
    let added = disks.added();
    assert_eq!(added.len(), 2);
    assert_eq!(
        (
            added[1].copy.as_path(),
            added[1].source.as_deref(),
            added[1].index
        ),
        (copy_a.as_path(), None, Some(2))
    );
    assert!(orphan_copies(&env.saves(), &work).unwrap().is_empty());
    // Removed with its copy (not inserted).
    disks.forget_added(&copy_a, true).unwrap();
    assert!(!copy_a.exists());
    assert_eq!(disks.slots().len(), 2);
    drop(emu);
    // An adopted orphan is never matched to a source of the same size added later.
    assert_eq!(fs::read(&copy_b).unwrap(), b"bbbbbb");
    assert_eq!(fs::read(&a).unwrap(), b"aaaa");
}

fn beyond_what_the_core_takes(lib: &Path) {
    set_disk(Some("ext"));
    let env = Env::new(lib, "full");
    let game = env.file("Game/Disk 1.st", b"game");
    let work = env.saves().join("Game.disks");
    drop(env.open(env.game(&game), true));
    // Ten added disks recorded; the mock takes eight images: seven are restored, the rest are logged.
    let manifest = work.join(".emu-rust-disks");
    let mut text = fs::read_to_string(&manifest).unwrap();
    for i in 0..10 {
        fs::write(work.join(format!("add{i}.st")), b"x").unwrap();
        text.push_str(&format!(
            "added\tadd{i}.st\t\t1\t-\t00000001\t/nowhere{i}\n"
        ));
    }
    fs::write(&manifest, &text).unwrap();
    let mut emu = env.open(env.game(&game), true);
    assert_eq!(paths(&mut emu).len(), 8);
    assert_eq!(emu.disks().unwrap().added().len(), 10, "nothing forgotten");
    drop(emu);
    let after = fs::read_to_string(&manifest).unwrap();
    assert_eq!(after.lines().count(), text.lines().count());
}

fn without_disk_interface(lib: &Path) {
    set_disk(Some("ext"));
    let env = Env::new(lib, "no-interface");
    let game = env.file("Game/Disk 1.st", b"game");
    let extra = env.file("Extra.st", b"extra");
    let mut emu = env.open(env.game(&game), true);
    emu.disks().unwrap().add(&extra).unwrap();
    drop(emu);
    set_disk(None);
    let mut emu = env.open(env.game(&game), true);
    assert!(emu.disks().is_none());
    emu.run_frame(&mut NullInput, &mut NullSink).unwrap();
}

fn file_session(lib: &Path) {
    set_disk(Some("ext"));
    let env = Env::new(lib, "file");
    let rom = env.file("Desk.st", b"desk");
    let extra = env.file("Data.st", b"data");
    let mut emu = env.open(ContentSpec::File(rom.clone()), true);
    assert_eq!(emu.content_name(), Some("Desk"));
    emu.disks().unwrap().add(&extra).unwrap();
    drop(emu);
    let mut emu = env.open(ContentSpec::File(rom), true);
    let copy = env.saves().join("Desk.disks/Data.st");
    assert!(paths(&mut emu).contains(&copy));
    let added = emu.disks().unwrap().added();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].copy, copy);
}

fn basic_interface_never_deletes_a_possibly_inserted_copy(lib: &Path) {
    set_disk(Some("basic"));
    let env = Env::new(lib, "basic");
    let rom = env.file("Desk.st", b"desk");
    let extra = env.file("Data.st", b"data");
    let mut emu = env.open(ContentSpec::File(rom), true);
    let mut disks = emu.disks().unwrap();
    let index = disks.add(&extra).unwrap();
    disks.insert(index).unwrap();
    let copy = env.saves().join("Desk.disks/Data.st");
    let added = disks.added();
    assert_eq!(
        (added.len(), added[0].index),
        (1, None),
        "no paths from a basic interface"
    );
    assert!(matches!(
        disks.forget_added(&copy, true),
        Err(RuntimeError::DiskInserted)
    ));
    assert!(copy.is_file());
    disks.eject().unwrap();
    disks.forget_added(&copy, true).unwrap();
    assert!(!copy.exists());
}
