//! Real Atari ST tests (2.0): hatari + EmuTOS 1.4 headless (ADR 0007, `docs/design/v2.0-atari-st.md` §6).
//!
//! All tests are `#[ignore]`: run `scripts/fetch-test-assets.sh` then
//! `cargo test -p emu-runtime --test atarist_hatari -- --ignored`. Set `EMU_UPDATE_FINGERPRINTS=1` to (re)write
//! `tests/fingerprints/atarist-*.txt` after inspecting the PNGs in `target/test-output/atarist/`.
//! Floppies are generated here (tiny original GEMDOS programs wrapped by `tests/tools/mkst.py`), except
//! Nova Defense (MIT) fetched by the script.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    CollectSink, ContentSpec, Emulator, EmulatorOptions, InputBackend, MediaPolicy, MediaRules,
    MediaSet, MouseButton, NullInput, RetroKey, ScriptedInput,
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
    let dir = root().join("target/test-output/atarist").join(name);
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

/// FNV-1a of the file: only used to compare a file with itself before / after (no dev-dependency needed).
fn sha(path: &Path) -> u64 {
    fs::read(path)
        .unwrap()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// Fresh data directories with EmuTOS installed as `tos.img` (the name the core expects).
fn options(dir: &Path) -> EmulatorOptions {
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
    }
}

fn load(spec: ContentSpec, dir: &Path) -> Emulator {
    require(&core_path());
    Emulator::load_content(&core_path(), spec, options(dir)).expect("load hatari")
}

fn single(name: &str, image: &Path) -> ContentSpec {
    // Always a working copy: hatari writes into its images, even with a single floppy.
    ContentSpec::Media {
        set: MediaSet {
            name: name.into(),
            images: vec![image.to_path_buf()],
            playlist: None,
        },
        rules: MediaRules::new(MediaPolicy::WorkingCopy),
    }
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

// ---- generated floppies -------------------------------------------------------------------------------

/// A GEMDOS program (relocation table empty) around `text`.
fn prg(text: &[u8]) -> Vec<u8> {
    let mut v = vec![0x60, 0x1A];
    v.extend((text.len() as u32).to_be_bytes());
    v.extend([0u8; 20]); // data, bss, symbols, reserved, flags
    v.extend([0, 0]); // absflag
    v.extend(text);
    v.extend([0u8; 4]); // no relocation
    v
}

/// `Pterm0`: leaves the desktop to EmuTOS.
fn prg_exit() -> Vec<u8> {
    prg(&[0x3F, 0x3C, 0x00, 0x00, 0x4E, 0x41])
}

/// Creates `W.TXT` on the boot floppy (Fcreate / Fwrite / Fclose) then exits: forces hatari to write the image.
fn prg_writer() -> Vec<u8> {
    let mut t: Vec<u8> = Vec::new();
    let w = |t: &mut Vec<u8>, s: &[u8]| t.extend_from_slice(s);
    w(&mut t, &[0x3F, 0x3C, 0, 0]); // move.w #0,-(sp)        attributes
    let pea_name = t.len();
    w(&mut t, &[0x48, 0x7A, 0, 0]); // pea name(pc)
    w(&mut t, &[0x3F, 0x3C, 0x00, 0x3C, 0x4E, 0x41]); // Fcreate
    w(&mut t, &[0x50, 0x8F]); // addq.l #8,sp
    w(&mut t, &[0x36, 0x00]); // move.w d0,d3
    let pea_data = t.len();
    w(&mut t, &[0x48, 0x7A, 0, 0]); // pea data(pc)
    w(&mut t, &[0x2F, 0x3C, 0, 0, 0, 4]); // move.l #4,-(sp)
    w(&mut t, &[0x3F, 0x03]); // move.w d3,-(sp)
    w(&mut t, &[0x3F, 0x3C, 0x00, 0x40, 0x4E, 0x41]); // Fwrite
    w(&mut t, &[0x4F, 0xEF, 0x00, 0x0C]); // lea 12(sp),sp
    w(&mut t, &[0x3F, 0x03]); // move.w d3,-(sp)
    w(&mut t, &[0x3F, 0x3C, 0x00, 0x3E, 0x4E, 0x41]); // Fclose
    w(&mut t, &[0x58, 0x8F]); // addq.l #4,sp
    w(&mut t, &[0x3F, 0x3C, 0, 0, 0x4E, 0x41]); // Pterm0
    let name = t.len();
    w(&mut t, b"W.TXT\0\0");
    let data = t.len();
    w(&mut t, b"DATA");
    for (at, target) in [(pea_name, name), (pea_data, data)] {
        let disp = (target - (at + 2)) as u16;
        t[at + 2..at + 4].copy_from_slice(&disp.to_be_bytes());
    }
    prg(&t)
}

/// 720 KB floppy with `\AUTO\<NAME>.PRG` (the name is the file stem, which becomes the program's name).
fn make_floppy(dir: &Path, name: &str, program: &[u8]) -> PathBuf {
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

// ---- fingerprints ---------------------------------------------------------------------------------------

fn core_hash() -> String {
    fs::read_to_string(root().join("tests/cores/hatari_libretro.so.sha256"))
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn parse_prints(text: &str) -> Vec<(u64, u64)> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (mut frame, mut fp) = (None, None);
            for part in l.split_whitespace() {
                if let Some(v) = part.strip_prefix("frame=") {
                    frame = v.parse().ok();
                } else if let Some(v) = part.strip_prefix("fingerprint=") {
                    fp = u64::from_str_radix(v, 16).ok();
                }
            }
            (
                frame.unwrap_or_else(|| panic!("bad fingerprint line: {l}")),
                fp.unwrap_or_else(|| panic!("bad fingerprint line: {l}")),
            )
        })
        .collect()
}

/// Compares (or, with `EMU_UPDATE_FINGERPRINTS`, writes) `tests/fingerprints/<file>`.
fn check_fingerprints(file: &str, what: &str, prints: &[(u64, u64)]) {
    let path = root().join("tests/fingerprints").join(file);
    if std::env::var_os("EMU_UPDATE_FINGERPRINTS").is_some() {
        let mut out = format!("# {what}; core sha256 {}\n", core_hash());
        for (frame, fp) in prints {
            out.push_str(&format!("frame={frame} fingerprint={fp:016x}\n"));
        }
        fs::write(&path, out).unwrap();
        return;
    }
    let text = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing {}: inspect target/test-output/atarist/*/*.png then run with EMU_UPDATE_FINGERPRINTS=1",
            path.display()
        )
    });
    assert_eq!(
        parse_prints(&text),
        prints,
        "fingerprints of {file} differ; regenerate with EMU_UPDATE_FINGERPRINTS=1 if the change is legitimate"
    );
}

const DESKTOP_CHECKPOINTS: [u64; 3] = [300, 450, 600];

/// Boots EmuTOS from a floppy whose only program exits at once: the GEM desktop. Fingerprints at the checkpoints.
fn desktop_session(tag: &str) -> Vec<(u64, u64)> {
    let dir = out_dir(tag);
    let floppy = make_floppy(&dir, "exit", &prg_exit());
    let mut emu = load(single("desktop", &floppy), &dir.join("data"));
    let mut prints = Vec::new();
    for n in 1..=*DESKTOP_CHECKPOINTS.last().unwrap() {
        run(&mut emu, &mut NullInput, 1);
        if DESKTOP_CHECKPOINTS.contains(&n) {
            prints.push((n, emu.frame().fingerprint()));
            png(&emu, &dir, &format!("desktop-{n}"));
        }
    }
    prints
}

// ---- tests ------------------------------------------------------------------------------------------------

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn emutos_desktop_is_reproducible() {
    let _guard = lock();
    let a = desktop_session("desktop-a");
    let b = desktop_session("desktop-b");
    assert_eq!(a, b, "two boots of the same floppy must be identical");
    // The desktop keeps a few animated pixels in the menu bar (top right): checkpoints are not expected equal.
    assert_ne!(
        a[0].1, a[2].1,
        "still booting at 300 frames, desktop at 600"
    );
    check_fingerprints(
        "atarist-hatari-emutos.txt",
        "hatari + EmuTOS 1.4 (tos.img = etos192us), generated floppy with AUTO\\EXIT.PRG, no input",
        &a,
    );
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn desktop_av_and_input_capabilities() {
    let _guard = lock();
    let dir = out_dir("caps");
    let floppy = make_floppy(&dir, "exit", &prg_exit());
    let mut emu = load(single("caps", &floppy), &dir.join("data"));
    assert!(
        emu.system_info()
            .library_name
            .to_lowercase()
            .contains("hatari")
    );
    run(&mut emu, &mut NullInput, 120);
    assert!(emu.uses_keyboard(), "hatari polls the keyboard");
    assert!(emu.uses_mouse(), "hatari polls the mouse");
    let av = emu.av_info();
    assert!((av.timing.fps - 60.0).abs() < 1.0 || (av.timing.fps - 50.0).abs() < 1.0);
    assert!((av.timing.sample_rate - 44_100.0).abs() < 1.0 || av.timing.sample_rate > 8000.0);
    let (w, h) = (emu.frame().width(), emu.frame().height());
    assert!(w >= 320 && h >= 200, "frame {w}x{h}");
}

#[test]
#[ignore = "needs hatari, EmuTOS and Nova Defense: scripts/fetch-test-assets.sh"]
fn nova_defense_auto_program_shows_its_title() {
    let _guard = lock();
    let st = root().join("tests/roms/nova-defense.st");
    require(&st);
    let before = sha(&st);
    let desktop = desktop_session("nova-ref");
    let dir = out_dir("nova");
    let mut emu = load(single("nova-defense", &st), &dir.join("data"));
    let mut prints = Vec::new();
    // Checkpoints late enough for the title to be settled: at frame 900 the fingerprint varied between runs
    // (floppy loading time / blinking text).
    for n in 1..=1100u64 {
        run(&mut emu, &mut NullInput, 1);
        if [1000, 1100].contains(&n) {
            prints.push((n, emu.frame().fingerprint()));
            png(&emu, &dir, &format!("nova-{n}"));
        }
    }
    drop(emu);
    assert_ne!(
        prints[1].1,
        desktop.last().unwrap().1,
        "Nova Defense did not start: the desktop is showing"
    );
    assert_eq!(sha(&st), before, "the original floppy must stay untouched");
    check_fingerprints(
        "atarist-hatari-nova.txt",
        "hatari + EmuTOS 1.4 + Nova Defense (nova-defense.st, AUTO folder), no input",
        &prints,
    );
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn mouse_opens_the_desk_menu() {
    let _guard = lock();
    let dir = out_dir("mouse");
    let floppy = make_floppy(&dir, "exit", &prg_exit());
    let mut emu = load(single("mouse", &floppy), &dir.join("data"));
    run(&mut emu, &mut NullInput, 400);
    let idle = emu.frame().fingerprint();
    png(&emu, &dir, "idle");
    // Frames are counted from the session start: scripted ranges use `frame_count()` as index.
    let base = emu.frame_count();
    // Hard push to the top-left corner (the pointer is clamped there), then a small move back onto "Desk".
    let mut input = ScriptedInput::new()
        .mouse(-8, -8, 0, base..base + 80)
        .mouse(6, 2, 0, base + 90..base + 95);
    run(&mut emu, &mut input, 100);
    let hover = emu.frame().fingerprint();
    png(&emu, &dir, "hover");
    // GEM drops a menu as soon as the pointer is on its title: the picture must differ from the idle desktop.
    assert_ne!(hover, idle, "hovering Desk must drop the menu down");
    // Down onto "Desktop info...", then a left click opens its dialog.
    let base = emu.frame_count();
    let mut input = ScriptedInput::new().mouse(0, 2, 0, base..base + 6).mouse(
        0,
        0,
        1 << (MouseButton::Left as u8),
        base + 20..base + 32,
    );
    run(&mut emu, &mut input, 80);
    let dialog = emu.frame().fingerprint();
    png(&emu, &dir, "dialog");
    assert_ne!(
        dialog, hover,
        "a left click on the menu item must open a dialog"
    );
    assert_ne!(dialog, idle);
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn keyboard_alt_a_opens_drive_a() {
    // `doc/emudesk.txt` of EmuTOS 1.4: "hold the Alt key down and type X, e.g. Alt-C displays drive C".
    let _guard = lock();
    let dir = out_dir("keyboard");
    let floppy = make_floppy(&dir, "exit", &prg_exit());
    let mut emu = load(single("keyboard", &floppy), &dir.join("data"));
    run(&mut emu, &mut NullInput, 400);
    let idle = emu.frame().fingerprint();
    png(&emu, &dir, "idle");
    let base = emu.frame_count();
    let alt = RetroKey::from_name("lalt").expect("lalt");
    let a = RetroKey::from_name("a").expect("a");
    let mut input = ScriptedInput::new()
        .key(alt, base + 5..base + 40)
        .key(a, base + 10..base + 25);
    run(&mut emu, &mut input, 100);
    png(&emu, &dir, "after");
    assert_ne!(
        emu.frame().fingerprint(),
        idle,
        "Alt-A must open the window of drive A"
    );
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn two_floppies_generated_playlist_and_swap() {
    let _guard = lock();
    let dir = out_dir("multi");
    let a = make_floppy(&dir, "diska", &prg_exit());
    let b = make_floppy(&dir, "diskb", &prg_exit());
    let (ha, hb) = (sha(&a), sha(&b));
    assert_ne!(ha, hb, "the two floppies must differ");
    let spec = ContentSpec::Media {
        set: MediaSet {
            name: "two disks".into(),
            images: vec![a.clone(), b.clone()],
            playlist: None,
        },
        rules: MediaRules::new(MediaPolicy::WorkingCopy),
    };
    let mut emu = load(spec, &dir.join("data"));
    let prepared = emu.prepared_media().expect("prepared media").clone();
    assert_eq!(prepared.images.len(), 2);
    assert_eq!(
        prepared.content.extension().and_then(|e| e.to_str()),
        Some("m3u"),
        "two images give a generated playlist"
    );
    let m3u = fs::read_to_string(&prepared.content).unwrap();
    let lines: Vec<&str> = m3u.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "playlist: {m3u:?}");
    assert!(
        lines.iter().all(|l| !Path::new(l).is_absolute()),
        "working-copy playlist uses relative names: {m3u:?}"
    );
    run(&mut emu, &mut NullInput, 500);
    {
        let mut disks = emu.disks().expect("hatari has a disk control");
        let slots = disks.slots();
        assert_eq!(slots.len(), 2);
        assert_ne!(slots[0].label, slots[1].label, "labels: {slots:?}");
        assert!(
            slots[0].label.contains("diska"),
            "label {:?}",
            slots[0].label
        );
        assert!(
            slots[1].label.contains("diskb"),
            "label {:?}",
            slots[1].label
        );
        assert_eq!(disks.current(), Some(0));
    }
    png(&emu, &dir, "disk-a");
    let next = emu.disks().unwrap().insert_next().expect("insert_next");
    assert_eq!(next, 1);
    run(&mut emu, &mut NullInput, 120);
    assert_eq!(emu.disks().unwrap().current(), Some(1));
    let again = emu.disks().unwrap().insert_next().expect("wrap");
    assert_eq!(again, 0, "insert_next wraps around");
    run(&mut emu, &mut NullInput, 60);
    drop(emu);
    assert_eq!(sha(&a), ha, "original A untouched");
    assert_eq!(sha(&b), hb, "original B untouched");
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn working_copy_is_written_and_original_untouched() {
    let _guard = lock();
    let dir = out_dir("workcopy");
    let floppy = make_floppy(&dir, "writer", &prg_writer());
    let before = sha(&floppy);
    let mut emu = load(single("writer", &floppy), &dir.join("data"));
    run(&mut emu, &mut NullInput, 400);
    let copy = emu.prepared_media().unwrap().images[0].clone();
    assert_ne!(copy, floppy);
    assert!(copy.starts_with(dir.join("data/saves")), "copy at {copy:?}");
    // hatari flushes the image when the floppy is ejected or at unload.
    emu.disks().unwrap().eject().ok();
    run(&mut emu, &mut NullInput, 30);
    drop(emu);
    assert_eq!(sha(&floppy), before, "the original must stay intact");
    let bytes = fs::read(&copy).unwrap();
    assert!(
        bytes.windows(11).any(|w| w == b"W       TXT"),
        "the program's file W.TXT is missing from the working copy"
    );
    assert_ne!(sha(&copy), before, "the working copy was not modified");
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn starts_without_content() {
    let _guard = lock();
    let dir = out_dir("nogame");
    let mut emu = load(ContentSpec::NoGame, &dir.join("data"));
    run(&mut emu, &mut NullInput, 1200);
    png(&emu, &dir, "desktop");
    assert_eq!(emu.game_name(), emu_runtime::NO_GAME_NAME);
    // Without a floppy EmuTOS waits for the drive: the GEM desktop (several colours) shows up after ~1000 frames.
    let colours: std::collections::HashSet<[u8; 4]> = emu
        .frame()
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .collect();
    assert!(colours.len() >= 3, "no desktop: {} colours", colours.len());
    assert!(
        emu.disks().is_some(),
        "disk menu available for adding floppies later"
    );
}
