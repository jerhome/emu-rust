//! Real cap32 test of the UI layer: a CPC game stored as a `.zip` must start its first floppy, not the empty
//! « Ready » screen (cap32 inserts nothing from a zip itself; `content_spec` makes the runtime extract it).
//!
//! `#[ignore]`: `scripts/fetch-test-assets.sh --with-5.0`, then
//! `cargo test -p emu-ui --test cpc_zip_real -- --ignored --test-threads=1`. Floppies are generated
//! (`tests/tools/mkcpc.py`, public domain); PNGs land in `target/test-output/ui-cpc-zip/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use emu_assets::cores::CorePreferences;
use emu_runtime::{CollectSink, Emulator, EmulatorOptions, NullInput, SaveScope};
use emu_ui::{GameContent, content_spec};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn out_dir(name: &str) -> PathBuf {
    let dir = root().join("target/test-output/ui-cpc-zip").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create output dir");
    dir
}

fn python(args: &[&str]) {
    let run = Command::new("python3")
        .arg("-I")
        .args(args)
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .output()
        .expect("python3 is required");
    assert!(
        run.status.success(),
        "python failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

/// Stores `(entry name, source file)` pairs in this order, uncompressed.
fn zip_of(zip: &Path, entries: &[(&str, &Path)]) {
    let mut args = vec![
        "-c".to_owned(),
        "import sys,zipfile\nz=zipfile.ZipFile(sys.argv[1],'w',zipfile.ZIP_STORED)\n\
         for a in sys.argv[2:]:\n n,p=a.split('=',1)\n z.write(p,n)\nz.close()"
            .to_owned(),
        zip.to_str().unwrap().to_owned(),
    ];
    args.extend(
        entries
            .iter()
            .map(|(n, p)| format!("{n}={}", p.to_str().unwrap())),
    );
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    python(&refs);
}

fn options(data: &Path) -> EmulatorOptions {
    for sub in ["system", "saves", "states", "assets"] {
        fs::create_dir_all(data.join(sub)).unwrap();
    }
    EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        cache_dir: data.join("cache"),
        provide_vfs: false,
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: Vec::new(),
        restore_added_disks: false,
        core_save_scope: SaveScope::Game,
        fastforward_override: true,
        variable_state_size: false,
        port_devices: Vec::new(),
    }
}

/// The UI path: `content_spec("cap32", Files)` with the built-in preferences, then the real core.
fn screen(content: &GameContent, dir: &Path, tag: &str) -> u64 {
    let core = root().join("tests/cores/cap32_libretro.so");
    assert!(
        core.is_file(),
        "missing {}: lancer scripts/fetch-test-assets.sh --with-5.0",
        core.display()
    );
    let spec = content_spec("cap32", content, &CorePreferences::builtin());
    let mut emu =
        Emulator::load_content(&core, spec, options(&dir.join(tag).join("data"))).expect("load");
    let mut sink = CollectSink::default();
    for _ in 0..600 {
        emu.run_frame(&mut NullInput, &mut sink).expect("run_frame");
    }
    emu.frame()
        .write_png(&dir.join(format!("{tag}.png")))
        .expect("png");
    emu.frame().fingerprint()
}

fn files(p: &[&Path]) -> GameContent {
    GameContent::Files(p.iter().map(|p| p.to_path_buf()).collect())
}

#[test]
#[ignore = "needs cap32: scripts/fetch-test-assets.sh --with-5.0"]
fn cpc_zip_starts_the_first_floppy_like_the_bare_dsk() {
    let dir = out_dir("run");
    let set = dir.join("set");
    python(&[
        root().join("tests/tools/mkcpc.py").to_str().unwrap(),
        "--set",
        set.to_str().unwrap(),
    ]);
    let (d1, d2, d3, d4) = (
        set.join("hello.dsk"),
        set.join("keys.dsk"),
        set.join("two.dsk"),
        set.join("faces (Side A).dsk"),
    );

    let bare = screen(&files(&[&d1]), &dir, "bare");
    let ready = screen(&GameContent::NoGame, &dir, "ready");
    assert_ne!(
        bare, ready,
        "the bare disk must differ from the Ready screen"
    );

    // One floppy.
    let one = dir.join("one.zip");
    zip_of(&one, &[("Game (1991).dsk", &d1)]);
    assert_eq!(screen(&files(&[&one]), &dir, "zip1"), bare, "1-disk zip");

    // TOSEC set stored out of order (Disk 2 before Disk 1).
    let two = dir.join("two-disks.zip");
    zip_of(
        &two,
        &[
            ("Game (1991)(Ubi)(Disk 2 of 2).dsk", &d2),
            ("Game (1991)(Ubi)(Disk 1 of 2).dsk", &d1),
        ],
    );
    assert_eq!(screen(&files(&[&two]), &dir, "zip2"), bare, "2-disk zip");

    let four = dir.join("four-disks.zip");
    zip_of(
        &four,
        &[
            ("Game (1991)(Ubi)(Disk 4 of 4).dsk", &d4),
            ("Game (1991)(Ubi)(Disk 2 of 4).dsk", &d2),
            ("Game (1991)(Ubi)(Disk 3 of 4).dsk", &d3),
            ("Game (1991)(Ubi)(Disk 1 of 4).dsk", &d1),
        ],
    );
    assert_eq!(screen(&files(&[&four]), &dir, "zip4"), bare, "4-disk zip");
}
