//! Real Amiga tests (3.0): puae headless with an **empty system directory** (the AROS ROM built into the core), on
//! generated or free disks only (`docs/design/v3.0-amiga.md` §6). No Kickstart anywhere.
//!
//! All tests are `#[ignore]`: run `scripts/fetch-test-assets.sh` then
//! `cargo test -p emu-runtime --test amiga_puae -- --ignored`. Set `EMU_UPDATE_FINGERPRINTS=1` to (re)write
//! `tests/fingerprints/amiga-puae.txt` after inspecting the PNGs in `target/test-output/amiga/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    CollectSink, ContentSpec, Emulator, EmulatorOptions, InputBackend, MediaPolicy, MediaRules,
    MediaSet, NullInput,
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
    root().join("tests/cores/puae_libretro.so")
}

fn out_dir(name: &str) -> PathBuf {
    let dir = root().join("target/test-output/amiga").join(name);
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

/// Fresh data directories; the system directory stays empty (AROS).
fn options(dir: &Path) -> EmulatorOptions {
    for sub in ["system", "saves", "states", "assets"] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    EmulatorOptions {
        system_dir: dir.join("system"),
        save_dir: dir.join("saves"),
        cache_dir: dir.join("cache"),
        provide_vfs: false,
        state_dir: dir.join("states"),
        assets_dir: dir.join("assets"),
        core_variables: Vec::new(),
        restore_added_disks: false,
        core_save_scope: emu_runtime::SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
        port_devices: Vec::new(),
    }
}

/// The rules of `cores.toml` for puae.
fn rules() -> MediaRules {
    MediaRules {
        policy: MediaPolicy::WorkingCopy,
        media_direct: vec!["ipf".into(), "lha".into()],
        max_copy_bytes: emu_runtime::MAX_COPY_BYTES,
        m3u_launch: emu_runtime::M3uLaunch::None,
    }
}

fn media(name: &str, images: &[&Path]) -> ContentSpec {
    ContentSpec::Media {
        set: MediaSet {
            name: name.into(),
            images: images.iter().map(|p| p.to_path_buf()).collect(),
            playlist: None,
            launch: None,
        },
        rules: rules(),
    }
}

fn load(spec: ContentSpec, dir: &Path) -> Emulator {
    require(&core_path());
    // puae uses `sinf` without linking libm itself: the host process must have it loaded (emu-app does, through
    // egui). This test binary would not link libm otherwise, hence the dynamic loader error `undefined symbol:
    // sinf`.
    assert!(std::hint::black_box(0.5f32).sin() > 0.0);
    Emulator::load_content(&core_path(), spec, options(dir)).expect("load puae")
}

fn run(emu: &mut Emulator, input: &mut dyn InputBackend, frames: u64) {
    let mut audio = CollectSink::default();
    for _ in 0..frames {
        emu.run_frame(input, &mut audio).expect("run_frame");
    }
}

fn png(emu: &Emulator, dir: &Path, tag: &str) {
    emu.frame()
        .write_png(&dir.join(format!("{tag}.png")))
        .expect("write png");
}

fn blank_adf(dir: &Path, label: &str) -> PathBuf {
    let path = dir.join(format!("{label}.adf"));
    let status = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools/mkadf.py"))
        .args(["--label", label])
        .arg(&path)
        .status()
        .expect("python3 is needed to run mkadf.py");
    assert!(status.success(), "mkadf.py failed");
    path
}

fn boot_adf() -> PathBuf {
    let p = root().join("tests/roms/amiga/boot.adf");
    require(&p);
    p
}

fn atk_adf() -> PathBuf {
    let p = root().join("tests/roms/amiga/AmigaTestKit-1.21.adf");
    require(&p);
    p
}

// ---- fingerprints (same file format as `atarist_hatari.rs`) ----------------------------------------------

fn core_hash() -> String {
    fs::read_to_string(root().join("tests/cores/puae_libretro.so.sha256"))
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// `name frame=N fingerprint=H` lines.
fn parse_prints(text: &str) -> Vec<(String, u64, u64)> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut parts = l.split_whitespace();
            let name = parts.next().unwrap().to_owned();
            let (mut frame, mut fp) = (None, None);
            for part in parts {
                if let Some(v) = part.strip_prefix("frame=") {
                    frame = v.parse().ok();
                } else if let Some(v) = part.strip_prefix("fingerprint=") {
                    fp = u64::from_str_radix(v, 16).ok();
                }
            }
            (
                name,
                frame.unwrap_or_else(|| panic!("bad fingerprint line: {l}")),
                fp.unwrap_or_else(|| panic!("bad fingerprint line: {l}")),
            )
        })
        .collect()
}

/// One file for the whole binary, filled by every test; compared (or, with `EMU_UPDATE_FINGERPRINTS`, merged)
/// by name so that tests may run alone.
fn check_fingerprint(name: &str, frame: u64, fp: u64) {
    let path = root().join("tests/fingerprints/amiga-puae.txt");
    let mut known = fs::read_to_string(&path)
        .map(|t| parse_prints(&t))
        .unwrap_or_default();
    if std::env::var_os("EMU_UPDATE_FINGERPRINTS").is_some() {
        known.retain(|(n, _, _)| n != name);
        known.push((name.to_owned(), frame, fp));
        known.sort();
        let mut out = format!(
            "# puae with an empty system directory (built-in AROS), generated or free disks, no input; core sha256 {}\n",
            core_hash()
        );
        for (n, f, p) in &known {
            out.push_str(&format!("{n} frame={f} fingerprint={p:016x}\n"));
        }
        fs::write(&path, out).unwrap();
        return;
    }
    let want = known
        .iter()
        .find(|(n, _, _)| n == name)
        .unwrap_or_else(|| {
            panic!(
                "no fingerprint `{name}` in {}: inspect target/test-output/amiga/*/*.png then run with EMU_UPDATE_FINGERPRINTS=1",
                path.display()
            )
        });
    assert_eq!(
        (want.1, want.2),
        (frame, fp),
        "fingerprint `{name}` differs; regenerate with EMU_UPDATE_FINGERPRINTS=1 if the change is legitimate"
    );
}

/// Runs `frames` frames and returns the fingerprint of the last one (PNG kept for inspection).
fn boot(tag: &str, spec: ContentSpec, frames: u64) -> (Emulator, u64, PathBuf) {
    let dir = out_dir(tag);
    let mut emu = load(spec, &dir.join("data"));
    run(&mut emu, &mut NullInput, frames);
    png(&emu, &dir, &format!("{tag}-{frames}"));
    let fp = emu.frame().fingerprint();
    (emu, fp, dir)
}

// ---- tests ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "needs puae and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn boot_adf_shows_the_rainbow_and_is_reproducible() {
    let _guard = lock();
    let adf = boot_adf();
    let before = sha(&adf);
    let (emu, fp_a, _) = boot("boot-a", media("boot", &[&adf]), 900);
    assert!(
        emu.system_info()
            .library_name
            .to_lowercase()
            .contains("puae"),
        "{}",
        emu.system_info().library_name
    );
    assert!(emu.uses_keyboard() || emu.uses_mouse() || emu.frame().width() > 0);
    drop(emu);
    let (_, fp_b, _) = boot("boot-b", media("boot", &[&adf]), 900);
    assert_eq!(fp_a, fp_b, "two boots of the same disk must be identical");
    check_fingerprint("boot-adf", 900, fp_a);
    assert_eq!(sha(&adf), before, "the original disk must stay untouched");
}

#[test]
#[ignore = "needs puae and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn amiga_test_kit_starts() {
    let _guard = lock();
    let adf = atk_adf();
    let before = sha(&adf);
    let (emu, fp, _) = boot("atk", media("atk", &[&adf]), 900);
    drop(emu);
    let (_, boot_fp, _) = boot("atk-vs-boot", media("boot", &[&boot_adf()]), 900);
    assert_ne!(
        fp, boot_fp,
        "the test kit shows something else than the bars"
    );
    check_fingerprint("amiga-test-kit", 900, fp);
    assert_eq!(sha(&adf), before);
}

#[test]
#[ignore = "needs puae: scripts/fetch-test-assets.sh"]
fn no_game_shows_the_aros_screen() {
    let _guard = lock();
    let dir = out_dir("aros");
    let mut emu = load(ContentSpec::NoGame, &dir.join("data"));
    run(&mut emu, &mut NullInput, 1000);
    png(&emu, &dir, "aros-1000");
    let fp = emu.frame().fingerprint();
    // « Waiting for bootable media » is drawn in hires interlaced: a taller frame than the bars of `boot.adf`.
    assert!(
        emu.frame().height() >= 400,
        "frame {}x{}",
        emu.frame().width(),
        emu.frame().height()
    );
    drop(emu);
    let (_, boot_fp, _) = boot("aros-vs-boot", media("boot", &[&boot_adf()]), 900);
    assert_ne!(fp, boot_fp);
    check_fingerprint("aros-no-game", 1000, fp);
}

#[test]
#[ignore = "needs puae and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn two_disks_df0_and_df1_work_on_copies() {
    let _guard = lock();
    let dir = out_dir("two");
    let boot = boot_adf();
    let blank = blank_adf(&dir, "SECOND");
    let (b0, k0) = (sha(&boot), sha(&blank));

    let mut emu = load(media("two", &[&boot, &blank]), &dir.join("data"));
    let prepared = emu.prepared_media().expect("working copies").clone();
    let work = prepared.work_dir.clone().expect("work dir");
    assert_eq!(prepared.images.len(), 2);
    assert!(
        prepared
            .images
            .iter()
            .all(|p| p.starts_with(&work) && p.is_file())
    );
    assert_eq!(
        prepared.content.extension().and_then(|e| e.to_str()),
        Some("m3u")
    );

    run(&mut emu, &mut NullInput, 900);
    png(&emu, &dir, "two-900");
    let first = emu.frame().fingerprint();
    {
        let mut disks = emu.disks().expect("puae has a disk control");
        let slots = disks.slots();
        assert_eq!(slots.len(), 2, "{slots:?}");
        assert_eq!(slots[0].path.as_deref(), Some(prepared.images[0].as_path()));
        assert_eq!(slots[1].path.as_deref(), Some(prepared.images[1].as_path()));
        assert_eq!(disks.current(), Some(0), "DF0 holds the first disk");
        assert_eq!(
            disks.insert_next().unwrap(),
            1,
            "DF0 now holds the second disk"
        );
        assert_eq!(disks.current(), Some(1));
    }
    run(&mut emu, &mut NullInput, 120);
    {
        let mut disks = emu.disks().unwrap();
        assert_eq!(disks.insert_next().unwrap(), 0, "wraps to the first disk");
    }
    run(&mut emu, &mut NullInput, 30);
    drop(emu);

    // The rainbow of the first disk is on screen (a blank disk in DF0 would leave the AROS « insert disk » eyes):
    // pinned, the picture differs from the single-disk boot only by the grey band the second drive's startup leaves.
    check_fingerprint("two-disks-df0-first", 900, first);
    // Originals untouched, whatever the core wrote.
    assert_eq!((sha(&boot), sha(&blank)), (b0, k0));
}
