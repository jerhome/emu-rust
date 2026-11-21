//! Real identity tests (2.1, `docs/design/v2.1-identite-disques.md` §5): hatari + EmuTOS, headless.
//!
//! All tests are `#[ignore]`: run `scripts/fetch-test-assets.sh` then
//! `cargo test -p emu-runtime --test identity_hatari -- --ignored --test-threads=1`.
//! Floppies are generated with `tests/tools/mkst.py` (tiny original GEMDOS programs).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    CollectSink, ContentSpec, Emulator, EmulatorOptions, MediaPolicy, MediaSet, NullInput,
};

/// One libretro core per process (ADR 0008): tests must not overlap.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn core_path() -> PathBuf {
    root().join("tests/cores/hatari_libretro.so")
}

fn out_dir(name: &str) -> PathBuf {
    let dir = root().join("target/test-output/identity").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create output dir");
    dir
}

fn require(path: &Path) {
    assert!(
        path.is_file(),
        "missing test asset {}: lancer scripts/fetch-test-assets.sh",
        path.display()
    );
}

/// FNV-1a of the file: only used to compare a file with itself before / after.
fn sha(path: &Path) -> u64 {
    fs::read(path)
        .unwrap()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// Data directories under `dir` (kept across sessions: the identity registry lives in them), EmuTOS as `tos.img`.
fn options(dir: &Path, restore_added_disks: bool) -> EmulatorOptions {
    let tos = root().join("tests/roms/system/tos.img");
    require(&tos);
    let system = dir.join("system");
    fs::create_dir_all(&system).unwrap();
    fs::copy(&tos, system.join("tos.img")).unwrap();
    for sub in ["saves", "states", "assets"] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    EmulatorOptions {
        system_dir: system,
        save_dir: dir.join("saves"),
        state_dir: dir.join("states"),
        assets_dir: dir.join("assets"),
        core_variables: Vec::new(),
        restore_added_disks,
    }
}

fn session(image: &Path, data: &Path, restore: bool) -> Emulator {
    require(&core_path());
    let spec = ContentSpec::Media {
        set: MediaSet {
            name: image.file_stem().unwrap().to_string_lossy().into_owned(),
            images: vec![image.to_path_buf()],
            playlist: None,
        },
        rules: emu_runtime::MediaRules::new(MediaPolicy::WorkingCopy),
    };
    Emulator::load_content(&core_path(), spec, options(data, restore)).expect("load hatari")
}

fn run(emu: &mut Emulator, frames: u64) {
    let mut audio = CollectSink::default();
    for _ in 0..frames {
        emu.run_frame(&mut NullInput, &mut audio)
            .expect("run_frame");
    }
}

/// A GEMDOS program (relocation table empty) around `text`.
fn prg(text: &[u8]) -> Vec<u8> {
    let mut v = vec![0x60, 0x1A];
    v.extend((text.len() as u32).to_be_bytes());
    v.extend([0u8; 20]);
    v.extend([0, 0]);
    v.extend(text);
    v.extend([0u8; 4]);
    v
}

/// `Pterm0`, optionally preceded by `nops` NOPs so that two floppies differ.
fn prg_exit(nops: usize) -> Vec<u8> {
    let mut t = [0x4E, 0x71].repeat(nops);
    t.extend([0x3F, 0x3C, 0x00, 0x00, 0x4E, 0x41]);
    prg(&t)
}

fn make_floppy(dir: &Path, name: &str, program: &[u8]) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let prg_path = dir.join(format!("{name}.prg"));
    fs::write(&prg_path, program).unwrap();
    let st = dir.join(format!("{name}.st"));
    let status = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools/mkst.py"))
        .arg(&st)
        .arg(&prg_path)
        .status()
        .expect("python3 is needed to run mkst.py");
    assert!(status.success(), "mkst.py failed");
    st
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn homonymous_floppies_get_separate_identities_and_files() {
    let _guard = lock();
    let dir = out_dir("homonyms");
    let first = make_floppy(&dir.join("Jeu1"), "disk1", &prg_exit(0));
    let second = make_floppy(&dir.join("Jeu2"), "disk1", &prg_exit(3));
    let (h1, h2) = (sha(&first), sha(&second));
    let data = dir.join("data");

    let mut emu = session(&first, &data, false);
    run(&mut emu, 120);
    let name1 = emu.identity().save_name.clone();
    let srm1 = emu.sram_path();
    let state1 = emu.save_state(0).expect("save state 1");
    let work1 = emu.prepared_media().unwrap().work_dir.clone().unwrap();
    drop(emu);

    let mut emu = session(&second, &data, false);
    run(&mut emu, 120);
    let name2 = emu.identity().save_name.clone();
    let srm2 = emu.sram_path();
    let state2 = emu.save_state(0).expect("save state 2");
    let work2 = emu.prepared_media().unwrap().work_dir.clone().unwrap();
    drop(emu);

    assert_eq!(name1, "disk1");
    assert_eq!(name2, "disk1 (2)");
    assert_ne!(srm1, srm2, "distinct .srm");
    assert_ne!(state1, state2, "distinct states");
    assert!(state1.is_file() && state2.is_file());
    assert_ne!(work1, work2, "distinct working copies");
    assert!(work1.is_dir() && work2.is_dir());

    // The same game keeps its identity at the next launch.
    let emu = session(&first, &data, false);
    assert_eq!(emu.identity().save_name, "disk1");
    assert_eq!(emu.state_path(0).unwrap(), state1);
    drop(emu);

    assert_eq!(sha(&first), h1, "first original untouched");
    assert_eq!(sha(&second), h2, "second original untouched");
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn added_disks_are_restored_in_the_next_session() {
    let _guard = lock();
    let dir = out_dir("added");
    let game = make_floppy(&dir.join("Jeu"), "disk1", &prg_exit(0));
    let extra_a = make_floppy(&dir.join("Extra"), "extra-a", &prg_exit(1));
    let extra_b = make_floppy(&dir.join("Extra"), "extra-b", &prg_exit(2));
    let hashes = [sha(&game), sha(&extra_a), sha(&extra_b)];
    let data = dir.join("data");

    let mut emu = session(&game, &data, true);
    run(&mut emu, 60);
    let first_added = {
        let mut disks = emu.disks().expect("hatari has a disk interface");
        disks.add(&extra_a).expect("add a");
        disks.add(&extra_b).expect("add b");
        disks.added()
    };
    assert_eq!(first_added.len(), 2);
    run(&mut emu, 60);
    drop(emu);

    let mut emu = session(&game, &data, true);
    run(&mut emu, 60);
    let restored = emu.disks().expect("disk interface").added();
    assert!(!restored.is_empty(), "added disks restored");
    let copies =
        |v: &[emu_runtime::AddedDisk]| v.iter().map(|d| d.copy.clone()).collect::<Vec<_>>();
    assert_eq!(
        copies(&restored),
        copies(&first_added),
        "same copies, same order"
    );
    assert!(
        restored.iter().all(|d| d.index.is_some()),
        "all in the core list"
    );
    let slots = emu.disks().unwrap().slots();
    assert_eq!(slots.len(), 3, "game + 2 added");
    drop(emu);

    // Without the option nothing is restored.
    let mut emu = session(&game, &data, false);
    assert_eq!(emu.disks().unwrap().slots().len(), 1);
    drop(emu);

    assert_eq!(
        [sha(&game), sha(&extra_a), sha(&extra_b)],
        hashes,
        "originals untouched"
    );
}
