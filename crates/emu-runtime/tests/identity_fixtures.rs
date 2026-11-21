// The mock core only builds on Unix.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
#![cfg(unix)]
// `std::env::set_var` is `unsafe` (edition 2024): the mock core reads its protocol with getenv().
#![allow(unsafe_code)]
//! 2.1 « no loss » fixtures (`docs/design/v2.1-identite-disques.md` §2.2, §5): realistic 1.0 / 2.0 / rc.1 save
//! trees from `tests/tools/mksaves.py` replayed through `resolve_game_identity` + `load_content_as` on the mock
//! core.
//!
//! One `Emulator` per process at a time and the `MOCK_*` variables are process-global: tests take `SERIAL`.

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use emu_runtime::{
    ContentSpec, Emulator, EmulatorOptions, IdentityNotice, MediaPolicy, MediaSet, NullInput,
    NullSink, OrphanKind, RuntimeError, StateOrigin, core_id_from_path, identity_request_for,
    orphan_copies, remove_orphan_copies, resolve_game_identity,
};

static SERIAL: Mutex<()> = Mutex::new(());

const REGISTRY: &str = ".emu-rust-games";
const MANIFEST: &str = ".emu-rust-disks";

struct Fixture {
    root: PathBuf,
    lib: PathBuf,
    core_id: String,
}

impl Fixture {
    /// Fresh tree of `profile` (`1.0`, `2.0`, `rc1`) and the mock core; `MOCK_DISK=ext` for disk games.
    fn new(test: &str, profile: &str) -> Self {
        let lib = mock_core::build(mock_core::Variant::Default);
        let root = mock_core::output_dir()
            .join("identity-fixtures")
            .join(format!("{test}-{profile}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let core_id = core_id_from_path(&lib);
        let status = Command::new("python3")
            .args(["-I"])
            .arg(mock_core::workspace_root().join("tests/tools/mksaves.py"))
            .args([profile])
            .arg(&root)
            .arg(&core_id)
            .status()
            .expect("python3 is needed to generate the fixtures");
        assert!(status.success(), "mksaves.py {profile} failed");
        for name in [
            "MOCK_KEYBOARD",
            "MOCK_MOUSE",
            "MOCK_DISK_DUMP",
            "MOCK_NO_GAME",
        ] {
            // SAFETY: tests of this binary hold `SERIAL`; no other thread touches the environment.
            unsafe { std::env::remove_var(name) };
        }
        // SAFETY: as above.
        unsafe { std::env::set_var("MOCK_DISK", "ext") };
        Self { root, lib, core_id }
    }

    fn options(&self) -> EmulatorOptions {
        let data = self.root.join("data");
        EmulatorOptions {
            system_dir: data.join("system"),
            save_dir: data.join("saves"),
            state_dir: data.join("states"),
            assets_dir: data.join("assets"),
            core_variables: Vec::new(),
            restore_added_disks: true,
        }
    }

    fn rom(&self, rel: &str) -> PathBuf {
        self.root.join("roms").join(rel)
    }

    fn saves(&self) -> PathBuf {
        self.root.join("data/saves").join(&self.core_id)
    }

    fn states(&self) -> PathBuf {
        self.root.join("data/states").join(&self.core_id)
    }

    fn file_spec(&self, rel: &str) -> ContentSpec {
        ContentSpec::File(self.rom(rel))
    }

    fn disk_spec(&self, name: &str, rel: &str) -> ContentSpec {
        ContentSpec::Media {
            set: MediaSet {
                name: name.into(),
                images: vec![self.rom(rel)],
                playlist: None,
            },
            rules: emu_runtime::MediaRules::new(MediaPolicy::WorkingCopy),
        }
    }

    fn identity(&self, spec: &ContentSpec) -> emu_runtime::GameIdentity {
        let (saves, states) = (self.saves(), self.states());
        let req = identity_request_for(spec, &saves, &states);
        resolve_game_identity(&req).unwrap()
    }

    /// Resolves then loads `spec`, runs a few frames.
    fn open(&self, spec: ContentSpec) -> Emulator {
        let id = self.identity(&spec);
        let mut emu = Emulator::load_content_as(&self.lib, spec, id, self.options()).unwrap();
        for _ in 0..3 {
            emu.run_frame(&mut NullInput, &mut NullSink).unwrap();
        }
        emu
    }
}

/// (length, CRC32) of every regular file under `root`, by relative path.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (usize, u32)> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<PathBuf, (usize, u32)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let data = fs::read(&path).unwrap();
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                out.insert(rel, (data.len(), crc32fast::hash(&data)));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Every file of `before` is still there with the same content. The manifests are only checked for existence:
/// a session may legitimately refresh them; the copies they describe are compared like everything else.
fn assert_nothing_lost(
    before: &BTreeMap<PathBuf, (usize, u32)>,
    after: &BTreeMap<PathBuf, (usize, u32)>,
) {
    for (path, hash) in before {
        let now = after
            .get(path)
            .unwrap_or_else(|| panic!("{} was removed or moved", path.display()));
        if path.file_name().is_some_and(|n| n == MANIFEST) {
            continue;
        }
        assert_eq!(now, hash, "{} was modified", path.display());
    }
}

fn data_only(snap: BTreeMap<PathBuf, (usize, u32)>) -> BTreeMap<PathBuf, (usize, u32)> {
    snap.into_iter()
        .filter(|(p, _)| p.starts_with("data"))
        .collect()
}

#[cfg(unix)]
#[test]
fn no_existing_file_is_renamed_moved_or_deleted() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for profile in ["1.0", "2.0", "rc1"] {
        let fx = Fixture::new("no-loss", profile);
        let before = snapshot(&fx.root);
        // Roms, saves and states alike: nothing of the tree may change except through explicit paths below.
        for rel in ["solo/Zelda.bin", "a/Twin.bin", "b/Twin.bin"] {
            let mut emu = fx.open(fx.file_spec(rel));
            let name = emu.identity().save_name.clone();
            emu.save_state(9).unwrap();
            drop(emu);
            assert!(fx.states().join(format!("{name}.state9")).is_file());
        }
        if profile != "1.0" {
            drop(fx.open(fx.disk_spec("Game", "st/Game.st")));
            drop(fx.open(fx.disk_spec("Game", "st2/Game.st")));
        }
        assert_nothing_lost(&before, &snapshot(&fx.root));
    }
}

#[cfg(unix)]
#[test]
fn non_homonym_adopts_its_files_as_they_are() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fx = Fixture::new("adopt", "1.0");
    let before = data_only(snapshot(&fx.root));
    let spec = fx.file_spec("solo/Zelda.bin");

    let id = fx.identity(&spec);
    assert_eq!((id.save_name.as_str(), id.index), ("Zelda", 1));
    assert_eq!(
        id.core_save_dir,
        fx.saves(),
        "index 1 keeps the core folder"
    );
    assert_eq!(id.notice, None);
    assert!(!id.inherits_legacy());
    // Resolution alone writes the registry (and its lock) and nothing else.
    let after = data_only(snapshot(&fx.root));
    assert_nothing_lost(&before, &after);
    for new in after.keys().filter(|p| !before.contains_key(*p)) {
        let name = new.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with(REGISTRY), "unexpected new file {name}");
    }
    assert_eq!(fx.identity(&spec), id, "idempotent");

    let emu = fx.open(spec);
    assert_eq!(emu.identity().save_name, "Zelda");
    assert_eq!(emu.sram_path(), fx.saves().join("Zelda.srm"));
    assert_eq!(emu.state_path(0).unwrap(), fx.states().join("Zelda.state0"));
    let sram = fs::read(fx.saves().join("Zelda.srm")).unwrap();
    assert_eq!(
        emu.memory(emu_runtime::MemoryKind::SaveRam),
        Some(&sram[..])
    );
    let (header, _) = emu.state_info(3).expect("existing state is listed");
    assert_eq!(header.game, "Zelda");
    assert_eq!(emu.state_origin(3), Some(StateOrigin::Unknown));
}

#[cfg(unix)]
#[test]
fn homonyms_are_separated_and_the_second_inherits_the_sram() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fx = Fixture::new("homonyms", "1.0");
    let shared_sram = fs::read(fx.saves().join("Twin.srm")).unwrap();
    let (a, b) = (fx.file_spec("a/Twin.bin"), fx.file_spec("b/Twin.bin"));

    let first = fx.identity(&a);
    assert_eq!((first.save_name.as_str(), first.index), ("Twin", 1));
    let second = fx.identity(&b);
    assert_eq!((second.save_name.as_str(), second.index), ("Twin (2)", 2));
    assert_eq!(second.core_save_dir, fx.saves().join("Twin (2)"));
    assert_eq!(second.base_name(), "Twin");
    assert_eq!(
        second.notice,
        Some(IdentityNotice::Homonym {
            // Recorded canonical, like the manifest's sources (`output_dir` contains `..`).
            first: fs::canonicalize(fx.rom("a/Twin.bin")).unwrap(),
            sram_copied: true,
            legacy_states: true,
        })
    );
    assert!(second.inherits_legacy());
    // `.srm` copied, never moved; states stay with the first.
    assert_eq!(
        fs::read(fx.saves().join("Twin (2).srm")).unwrap(),
        shared_sram
    );
    assert_eq!(fs::read(fx.saves().join("Twin.srm")).unwrap(), shared_sram);
    assert!(fx.states().join("Twin.state0").is_file());
    assert!(!fx.states().join("Twin (2).state0").exists());
    assert_eq!(
        (
            fx.identity(&a).save_name.as_str(),
            fx.identity(&b).save_name.as_str()
        ),
        ("Twin", "Twin (2)")
    );

    // The core sees one save folder each (the mock writes `mock-was-here.txt` there).
    drop(fx.open(a));
    drop(fx.open(b));
    assert!(fx.saves().join("mock-was-here.txt").is_file());
    assert!(
        fx.saves()
            .join("Twin (2)")
            .join("mock-was-here.txt")
            .is_file()
    );
    assert_eq!(fs::read(fx.saves().join("Twin.srm")).unwrap(), shared_sram);
}

#[cfg(unix)]
#[test]
fn a_state_of_another_content_is_flagged() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fx = Fixture::new("other-content", "1.0");

    // Same path, content replaced (patch, fixed dump): same identity, the old state no longer matches.
    let zelda = fx.file_spec("solo/Zelda.bin");
    let mut emu = fx.open(zelda.clone());
    emu.save_state(1).unwrap();
    assert_eq!(emu.state_origin(1), Some(StateOrigin::Same));
    drop(emu);
    fs::write(fx.rom("solo/Zelda.bin"), b"patched rom, other size").unwrap();
    let emu = fx.open(zelda);
    assert_eq!(emu.identity().save_name, "Zelda");
    assert_eq!(emu.state_origin(1), Some(StateOrigin::OtherContent));
    assert_eq!(
        emu.state_origin(0),
        Some(StateOrigin::Unknown),
        "1.0 state, no homonym"
    );
    assert_eq!(emu.state_origin(8), None, "empty slot");
    drop(emu);

    // A 2.1 state of Twin copied by hand to the homonym's name: not that game's.
    let (a, b) = (fx.file_spec("a/Twin.bin"), fx.file_spec("b/Twin.bin"));
    let _ = fx.identity(&a);
    let _ = fx.identity(&b);
    let mut emu_a = fx.open(a.clone());
    assert_eq!(
        emu_a.state_origin(0),
        Some(StateOrigin::Ambiguous),
        "1.0 state of a name carried by two games"
    );
    emu_a.save_state(2).unwrap();
    drop(emu_a);
    fs::copy(
        fx.states().join("Twin.state2"),
        fx.states().join("Twin (2).state2"),
    )
    .unwrap();
    let emu_b = fx.open(b);
    assert_eq!(emu_b.state_origin(2), Some(StateOrigin::OtherContent));
}

#[cfg(unix)]
#[test]
fn orphans_are_listed_and_only_removed_on_request() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fx = Fixture::new("orphans", "rc1");
    let work = fx.saves().join("Game.disks");
    let before = snapshot(&work);

    let mut emu = fx.open(fx.disk_spec("Game", "st/Game.st"));
    // The disk added in an earlier session is back in the core's list (design §2.3).
    let added = emu.disks().expect("disk control").added();
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!(added[0].copy, work.join("Extra.st"));
    assert_eq!(
        added[0].source,
        Some(fs::canonicalize(fx.rom("extra/Extra.st")).unwrap())
    );
    drop(emu);

    let orphans = orphan_copies(&fx.saves(), &work).unwrap();
    let kind = |name: &str| orphans.iter().find(|o| o.name == name).map(|o| o.kind);
    assert_eq!(kind("02 X.st"), Some(OrphanKind::Copy), "{orphans:?}");
    assert_eq!(
        kind("Game.m3u"),
        Some(OrphanKind::StalePlaylist),
        "{orphans:?}"
    );
    for referenced in ["Game.st", "Extra.st", MANIFEST] {
        assert_eq!(kind(referenced), None, "{referenced} is referenced");
    }
    // Listing, and a whole session, deleted nothing.
    assert_nothing_lost(&before, &snapshot(&work));

    // Refusals leave everything in place.
    for bad in ["Game.st", MANIFEST, "../Game.srm"] {
        let r = remove_orphan_copies(&fx.saves(), &work, &[bad.to_owned()]);
        assert!(
            matches!(r, Err(RuntimeError::RemovalRefused { .. })),
            "{bad}: {r:?}"
        );
    }
    assert!(remove_orphan_copies(&fx.saves(), &work, &["nope.st".to_owned()]).is_err());
    assert!(fx.saves().join("Game.srm").is_file());
    assert_nothing_lost(&before, &snapshot(&work));

    // Explicit request: only the ticked line goes.
    let freed = remove_orphan_copies(&fx.saves(), &work, &["02 X.st".to_owned()]).unwrap();
    assert_eq!(freed, before[&PathBuf::from("02 X.st")].0 as u64);
    let after = snapshot(&work);
    assert!(!after.contains_key(Path::new("02 X.st")));
    assert!(
        after.contains_key(Path::new("Game.m3u")),
        "unticked orphan kept"
    );
    assert_eq!(after.len(), before.len() - 1);
}

#[cfg(unix)]
#[test]
fn the_owner_of_game_disks_keeps_its_name_after_a_new_homonym() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fx = Fixture::new("owner-after-newcomer", "2.0");
    let before = snapshot(&fx.root);
    // A third `Game.st`, never played, is launched before the two 2.0 owners.
    let newcomer = fx.rom("st3/Game.st");
    fs::create_dir_all(newcomer.parent().unwrap()).unwrap();
    fs::write(&newcomer, b"a disk never played").unwrap();
    let id = fx.identity(&fx.disk_spec("Game", "st3/Game.st"));
    assert_eq!(
        id.save_name, "Game (3)",
        "Game.disks and Game (2).disks are taken"
    );
    let owner = fx.identity(&fx.disk_spec("Game", "st/Game.st"));
    assert_eq!(owner.save_name, "Game");
    let second = fx.identity(&fx.disk_spec("Game", "st2/Game.st"));
    assert_eq!(second.save_name, "Game (2)");
    // The owner gets its copies (with their progress) back.
    let mut emu = fx.open(fx.disk_spec("Game", "st/Game.st"));
    assert_eq!(
        emu.prepared_media().unwrap().work_dir.as_deref(),
        Some(fx.saves().join("Game.disks").as_path())
    );
    emu.run_frame(&mut NullInput, &mut NullSink).unwrap();
    drop(emu);
    assert_nothing_lost(&before, &snapshot(&fx.root));
}
