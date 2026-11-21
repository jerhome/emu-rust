//! Real Atari ST tests of the UI layer (2.0, `docs/design/v2.0-atari-st.md` §6, Q2): hatari + EmuTOS 1.4.
//!
//! All tests are `#[ignore]`: `scripts/fetch-test-assets.sh`, `cargo build -p emu-app`, then
//! `cargo test -p emu-ui --test atari_st -- --ignored --test-threads=1`.
//! Part 1 goes through `emu_ui::content_spec` (what the library launch uses) and the real core. Part 2 starts the
//! `emu-rust` binary on a private Xvfb and injects real X events with `tests/tools/xinject.py` (XTEST), so the host
//! key, the mode switch and the per-game options are exercised end to end. PNGs land in `target/test-output/ui-st/`.
//! Floppies are generated (tiny original GEMDOS programs wrapped by `tests/tools/mkst.py`), except Nova Defense (MIT).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use emu_assets::cores::CorePreferences;
use emu_runtime::{
    CollectSink, ContentSpec, Emulator, EmulatorOptions, InputBackend, JoypadButton, NullInput,
    RetroKey, ScriptedInput,
};
use emu_ui::{GameContent, content_spec};

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
    let dir = root().join("target/test-output/ui-st").join(name);
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

/// FNV-1a: only used to compare a file with itself before / after.
fn sha(path: &Path) -> u64 {
    fs::read(path)
        .unwrap()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// A data root laid out like the app's (`DataDirs::under`) with EmuTOS as `system/tos.img`.
fn data_root(dir: &Path) -> PathBuf {
    let tos = root().join("tests/roms/system/tos.img");
    require(&tos);
    let data = dir.join("data");
    fs::create_dir_all(data.join("system")).unwrap();
    fs::copy(&tos, data.join("system/tos.img")).unwrap();
    for sub in ["saves", "states", "assets", "roms"] {
        fs::create_dir_all(data.join(sub)).unwrap();
    }
    data
}

fn emu_options(data: &Path) -> EmulatorOptions {
    EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        cache_dir: data.join("cache"),
        provide_vfs: false,
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: Vec::new(),
        restore_added_disks: false,
    }
}

/// The library path: `GameContent` -> `content_spec` (hatari policy from the built-in preferences) -> runtime.
fn launch(content: &GameContent, data: &Path) -> Emulator {
    require(&core_path());
    let spec = content_spec("hatari", content, &CorePreferences::builtin());
    Emulator::load_content(&core_path(), spec, emu_options(data)).expect("load hatari")
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

fn colours(emu: &Emulator) -> usize {
    emu.frame()
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .collect::<std::collections::HashSet<[u8; 4]>>()
        .len()
}

// ---- generated floppies ---------------------------------------------------------------------------------

/// A GEMDOS program (empty relocation table) around `text`.
fn prg(text: &[u8]) -> Vec<u8> {
    let mut v = vec![0x60, 0x1A];
    v.extend((text.len() as u32).to_be_bytes());
    v.extend([0u8; 20]);
    v.extend([0, 0]);
    v.extend(text);
    v.extend([0u8; 4]);
    v
}

fn prg_exit() -> Vec<u8> {
    prg(&[0x3F, 0x3C, 0x00, 0x00, 0x4E, 0x41])
}

/// Reads eight keys with `Crawcin`, then writes `KEYS` + eight big-endian longs (`00SS00AA`: scancode, ASCII) to
/// `K.TXT` on the boot floppy and exits.
fn prg_keyprobe() -> Vec<u8> {
    let mut t: Vec<u8> = Vec::new();
    let w = |t: &mut Vec<u8>, s: &[u8]| t.extend_from_slice(s);
    let lea_buf = t.len();
    w(&mut t, &[0x47, 0xFA, 0, 0]); // lea buf+4(pc),a3
    w(&mut t, &[0x78, 0x07]); // moveq #7,d4
    w(&mut t, &[0x3F, 0x3C, 0x00, 0x07, 0x4E, 0x41, 0x54, 0x8F]); // loop: Crawcin; addq.l #2,sp
    w(&mut t, &[0x26, 0xC0]); // move.l d0,(a3)+
    w(&mut t, &[0x51, 0xCC, 0xFF, 0xF4]); // dbra d4,loop
    w(&mut t, &[0x3F, 0x3C, 0, 0]); // attributes
    let pea_name = t.len();
    w(&mut t, &[0x48, 0x7A, 0, 0]);
    w(
        &mut t,
        &[0x3F, 0x3C, 0x00, 0x3C, 0x4E, 0x41, 0x50, 0x8F, 0x36, 0x00],
    ); // Fcreate; addq; move d0,d3
    let pea_buf = t.len();
    w(&mut t, &[0x48, 0x7A, 0, 0]);
    w(&mut t, &[0x2F, 0x3C, 0, 0, 0, 36, 0x3F, 0x03]);
    w(
        &mut t,
        &[0x3F, 0x3C, 0x00, 0x40, 0x4E, 0x41, 0x4F, 0xEF, 0x00, 0x0C],
    ); // Fwrite; lea 12(sp),sp
    w(
        &mut t,
        &[0x3F, 0x03, 0x3F, 0x3C, 0x00, 0x3E, 0x4E, 0x41, 0x58, 0x8F],
    ); // Fclose
    w(&mut t, &[0x3F, 0x3C, 0, 0, 0x4E, 0x41]); // Pterm0
    let name = t.len();
    w(&mut t, b"K.TXT\0");
    if t.len() % 2 == 1 {
        t.push(0); // the buffer is written with move.l: it must be word aligned
    }
    let buf = t.len();
    w(&mut t, b"KEYS");
    w(&mut t, &[0u8; 32]);
    for (at, target) in [(lea_buf, buf + 4), (pea_name, name), (pea_buf, buf)] {
        let disp = (target - (at + 2)) as u16;
        t[at + 2..at + 4].copy_from_slice(&disp.to_be_bytes());
    }
    prg(&t)
}

/// 720 KB floppy with `\AUTO\<NAME>.PRG`.
fn make_floppy(dir: &Path, file_stem: &str, program: &[u8]) -> PathBuf {
    let prg_path = dir.join("prog.prg");
    fs::write(&prg_path, program).unwrap();
    let st = dir.join(format!("{file_stem}.st"));
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

/// The five scancodes `K.TXT` of a (flushed) working copy holds, `None` when the file is missing.
fn probe_scancodes(image: &Path) -> Option<Vec<u8>> {
    let bytes = fs::read(image).ok()?;
    bytes
        .windows(36)
        .rfind(|w| &w[..4] == b"KEYS" && w[4..].iter().any(|b| *b != 0))
        .map(|w| w[4..].chunks(4).map(|l| l[1]).collect::<Vec<u8>>())
}

// ---- part 1: the launch layer + the real core -----------------------------------------------------------

#[test]
#[ignore = "needs hatari, EmuTOS and Nova Defense: scripts/fetch-test-assets.sh"]
fn library_launch_of_a_game_uses_a_working_copy_even_with_one_disk() {
    let _guard = lock();
    let st = root().join("tests/roms/nova-defense.st");
    require(&st);
    let before = sha(&st);
    let dir = out_dir("launch");
    let data = data_root(&dir);
    let content = GameContent::Files(vec![st.clone()]);
    assert!(matches!(
        content_spec("hatari", &content, &CorePreferences::builtin()),
        ContentSpec::Media { .. }
    ));
    let mut emu = launch(&content, &data);
    run(&mut emu, &mut NullInput, 1100);
    png(&emu, &dir, "nova");
    assert!(colours(&emu) >= 3, "Nova Defense shows nothing");
    let copy = emu.prepared_media().expect("prepared media").images[0].clone();
    assert_ne!(copy, st, "the core must get a copy");
    assert!(copy.starts_with(data.join("saves")), "copy at {copy:?}");
    drop(emu);
    assert_eq!(sha(&st), before, "the original floppy must stay untouched");
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn desktop_without_content_boots_to_gem() {
    let _guard = lock();
    let dir = out_dir("desktop");
    let data = data_root(&dir);
    assert_eq!(
        content_spec("hatari", &GameContent::NoGame, &CorePreferences::builtin()),
        ContentSpec::NoGame
    );
    let mut emu = launch(&GameContent::NoGame, &data);
    run(&mut emu, &mut NullInput, 1200);
    png(&emu, &dir, "desktop");
    assert!(colours(&emu) >= 3, "no GEM desktop");
    assert!(emu.disks().is_some(), "disk menu must stay available");
}

#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn two_generated_disks_group_into_one_game_and_swap() {
    let _guard = lock();
    let dir = out_dir("twodisks");
    let data = data_root(&dir);
    let a = make_floppy(&dir, "Duo (1990)(Test)(Disk 1 of 2)", &prg_exit());
    let b = make_floppy(&dir, "Duo (1990)(Test)(Disk 2 of 2)", &prg_keyprobe());
    let (ha, hb) = (sha(&a), sha(&b));
    assert_ne!(ha, hb);
    let content = GameContent::Files(vec![a.clone(), b.clone()]);
    assert_eq!(
        emu_ui::media_set_name(&[a.clone(), b.clone()]),
        "Duo (1990)(Test)"
    );
    let mut emu = launch(&content, &data);
    assert_eq!(emu.game_name(), "Duo (1990)(Test)");
    run(&mut emu, &mut NullInput, 500);
    {
        let mut disks = emu.disks().expect("disk control");
        assert_eq!(disks.slots().len(), 2);
        assert_eq!(disks.insert_next().expect("next"), 1);
        assert_eq!(disks.insert_next().expect("wrap"), 0);
    }
    run(&mut emu, &mut NullInput, 60);
    drop(emu);
    assert_eq!((sha(&a), sha(&b)), (ha, hb), "originals untouched");
}

/// Scancodes the real core hands to TOS for the keys the UI relies on (virtual keyboard, physical `<`).
#[test]
#[ignore = "needs hatari and EmuTOS: scripts/fetch-test-assets.sh"]
fn help_undo_and_iso_key_reach_tos_with_their_atari_scancodes() {
    let _guard = lock();
    let dir = out_dir("keyprobe");
    let data = data_root(&dir);
    let probe = make_floppy(&dir, "probe", &prg_keyprobe());
    let mut emu = launch(&GameContent::Files(vec![probe]), &data);
    run(&mut emu, &mut NullInput, 450);
    let base = emu.frame_count();
    let names = ["help", "undo", "less", "oem_102", "a", "b", "c", "d", "e"];
    let mut input = ScriptedInput::new();
    for (i, n) in names.iter().enumerate() {
        let key = RetroKey::from_name(n).unwrap_or_else(|| panic!("no RetroKey {n}"));
        let start = base + 10 + 60 * i as u64;
        // One character per key: a longer hold would trigger TOS key repeat.
        input = input.key(key, start..start + 10);
    }
    run(&mut emu, &mut input, 700);
    png(&emu, &dir, "after-keys");
    let copy = emu.prepared_media().unwrap().images[0].clone();
    emu.disks().unwrap().eject().ok();
    run(&mut emu, &mut NullInput, 30);
    drop(emu);
    let codes = probe_scancodes(&copy).expect("K.TXT not written: did the probe get eight keys?");
    eprintln!("scancodes for {names:?}: {codes:02x?}");
    // Atari scancodes: Help $62, Undo $61, ISO key (`<`) $60, then A $1E, B $30, C $2E, D $20, E $12.
    // hatari takes the ISO key as RETROK_LESS; RETROK_OEM_102 is ignored by the core (no character at all).
    assert_eq!(
        codes,
        vec![0x62, 0x61, 0x60, 0x1E, 0x30, 0x2E, 0x20, 0x12],
        "Help, Undo, less, (oem_102 ignored), a b c d e"
    );
}

/// Joystick fire: RetroPad B (what J drives in joystick mode) must act on a game; A (second button) is shown for
/// information (it starts Nova Defense too: both are fire for hatari). The title waits for fire.
#[test]
#[ignore = "needs hatari, EmuTOS and Nova Defense: scripts/fetch-test-assets.sh"]
fn retropad_b_fires_the_st_joystick() {
    let _guard = lock();
    let st = root().join("tests/roms/nova-defense.st");
    require(&st);
    let mut prints = Vec::new();
    for (tag, button) in [
        ("none", None),
        ("b", Some(JoypadButton::B)),
        ("a", Some(JoypadButton::A)),
    ] {
        let dir = out_dir(&format!("fire-{tag}"));
        let data = data_root(&dir);
        let mut emu = launch(&GameContent::Files(vec![st.clone()]), &data);
        run(&mut emu, &mut NullInput, 1100);
        let base = emu.frame_count();
        let mut input = ScriptedInput::new();
        if let Some(b) = button {
            input = input.press(0, b, base + 5..base + 30);
        }
        run(&mut emu, &mut input, 150);
        png(&emu, &dir, "after");
        prints.push((tag, emu.frame().fingerprint()));
        drop(emu);
    }
    eprintln!("fingerprints: {prints:x?}");
    assert_ne!(
        prints[1].1, prints[0].1,
        "RetroPad B did not change anything: fire is not on B"
    );
}

// ---- part 2: the binary under Xvfb ----------------------------------------------------------------------

struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    fn start() -> Xvfb {
        // -displayfd needs a pipe fd: let the shell hand fd 3 to a temp file.
        let tmp = std::env::temp_dir().join(format!("emu-ui-st-display-{}", std::process::id()));
        let _ = fs::remove_file(&tmp);
        let child = Command::new("bash")
            .arg("-c")
            .arg(format!(
                "exec Xvfb -displayfd 3 -screen 0 1280x800x24 -nolisten tcp 3>{} 2>/dev/null",
                tmp.display()
            ))
            .spawn()
            .expect("Xvfb is needed for the captures");
        let start = Instant::now();
        let display = loop {
            if let Ok(text) = fs::read_to_string(&tmp)
                && let Some(line) = text.lines().next().filter(|l| !l.is_empty())
            {
                break format!(":{line}");
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "Xvfb did not start"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        let _ = fs::remove_file(&tmp);
        Xvfb { child, display }
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn app_binary() -> PathBuf {
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), PathBuf::from);
    let bin = target.join("debug/emu-rust");
    assert!(
        bin.is_file(),
        "build the app first: cargo build -p emu-app ({})",
        bin.display()
    );
    bin
}

fn app(display: &str, args: &[&std::ffi::OsStr]) -> Command {
    let mut cmd = Command::new(app_binary());
    cmd.env_remove("WAYLAND_DISPLAY")
        .env("DISPLAY", display)
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    cmd
}

fn wait_exit(mut child: Child, limit: Duration) -> String {
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            let mut err = String::new();
            use std::io::Read;
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_string(&mut err);
            }
            return err;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            panic!("emu-rust did not exit within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn inject(display: &str, actions: &[&str]) {
    let status = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools/xinject.py"))
        .arg(display)
        .args(actions)
        .status()
        .expect("python3");
    assert!(status.success(), "xinject failed");
}

fn os(s: &str) -> &std::ffi::OsStr {
    std::ffi::OsStr::new(s)
}

/// Right Ctrl is never forwarded; Host+D inserts the next floppy; Host+J switches to joystick mode; the choices are
/// written to the per-game options file; the keyboard mode defaults to « keyboard » (the first plain key is typed).
#[test]
#[ignore = "needs hatari, EmuTOS, Xvfb, libXtst and a built emu-rust: scripts/fetch-test-assets.sh"]
fn host_key_swaps_disk_toggles_mode_and_writes_game_options() {
    let _guard = lock();
    let dir = out_dir("hostkey");
    let data = data_root(&dir);
    let a = make_floppy(&dir, "Duo (1990)(Test)(Disk 1 of 2)", &prg_keyprobe());
    let b = make_floppy(&dir, "Duo (1990)(Test)(Disk 2 of 2)", &prg_exit());
    let (ha, hb) = (sha(&a), sha(&b));
    let x = Xvfb::start();
    let core = core_path();
    let datadir = data.clone();
    let child = app(
        &x.display,
        &[
            os("--core"),
            core.as_os_str(),
            os("--rom"),
            a.as_os_str(),
            os("--rom"),
            b.as_os_str(),
            os("--data-dir"),
            datadir.as_os_str(),
            os("--no-audio"),
            os("--frames"),
            os("2400"),
        ],
    )
    .spawn()
    .expect("spawn emu-rust");
    // Boot (~450 frames = 9 s real time), then the probe waits for keys.
    std::thread::sleep(Duration::from_secs(14));
    inject(
        &x.display,
        &[
            "focus",
            "sleep",
            "0.5",
            // Host+D: next floppy (nothing must reach TOS)
            "down",
            "Control_R",
            "tap",
            "d",
            "up",
            "Control_R",
            "sleep",
            "0.5",
            // keyboard mode (default): keys are typed
            "tap",
            "x",
            "tap",
            "y",
            "tap",
            "z",
            "tap",
            "b",
            "tap",
            "n",
            "tap",
            "c",
            "tap",
            "v",
            "tap",
            "m",
            "sleep",
            "0.5",
            // Host+J: joystick mode, then the host key alone opens and closes the menu (options are saved)
            "down",
            "Control_R",
            "tap",
            "j",
            "up",
            "Control_R",
            "sleep",
            "0.5",
            "tap",
            "Control_R",
            "sleep",
            "1",
            "tap",
            "Control_R",
            "sleep",
            "1",
        ],
    );
    let stderr = wait_exit(child, Duration::from_secs(120));
    drop(x);
    let opts = data.join("game-options/hatari/Duo (1990)(Test).toml");
    let text = fs::read_to_string(&opts).unwrap_or_else(|e| {
        panic!(
            "no options file {}: {e}\n--- emu-rust stderr ---\n{stderr}",
            opts.display()
        )
    });
    eprintln!("{text}");
    assert!(
        text.contains("last_disk = 1"),
        "Host+D not recorded:\n{text}"
    );
    assert!(
        text.contains("keyboard_mode = \"joystick\""),
        "Host+J not recorded:\n{text}"
    );
    assert_eq!((sha(&a), sha(&b)), (ha, hb), "originals untouched");
    // The probe ran from the working copy of disk 1: the eight keys it saw are x y z b n c v m, no Ctrl (1D) / D (20).
    let copy = data.join("saves/hatari/Duo (1990)(Test).disks");
    let image = fs::read_dir(&copy)
        .unwrap_or_else(|e| panic!("{}: {e}", copy.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "st"))
        .expect("working copy of disk 1");
    let mut copies: Vec<_> = fs::read_dir(&copy)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    copies.sort();
    let codes = copies
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "st"))
        .find_map(|p| probe_scancodes(p))
        .unwrap_or_else(|| panic!("K.TXT not found in {copies:?} ({image:?})"));
    assert_eq!(
        codes,
        vec![0x2D, 0x15, 0x2C, 0x30, 0x31, 0x2E, 0x2F, 0x32],
        "scancodes seen by TOS (x y z b n c v m)"
    );
}

/// Each screen of the 2.0 interface renders (captures kept for visual review).
#[test]
#[ignore = "needs hatari, EmuTOS, Xvfb and a built emu-rust: scripts/fetch-test-assets.sh"]
fn st_screens_render() {
    let _guard = lock();
    let dir = out_dir("screens");
    let data = data_root(&dir);
    let a = make_floppy(&dir, "Duo (1990)(Test)(Disk 1 of 2)", &prg_exit());
    let b = make_floppy(&dir, "Duo (1990)(Test)(Disk 2 of 2)", &prg_exit());
    let x = Xvfb::start();
    for screen in ["vkbd", "disks", "computer"] {
        let shot = dir.join(format!("{screen}.png"));
        let child = app(
            &x.display,
            &[
                os("--core"),
                core_path().as_os_str(),
                os("--rom"),
                a.as_os_str(),
                os("--rom"),
                b.as_os_str(),
                os("--data-dir"),
                data.as_os_str(),
                os("--no-audio"),
                os("--frames"),
                os("60"),
                os("--screen"),
                os(screen),
                os("--dump-window"),
                shot.as_os_str(),
            ],
        )
        .spawn()
        .expect("spawn");
        let err = wait_exit(child, Duration::from_secs(90));
        assert!(
            fs::metadata(&shot).is_ok_and(|m| m.len() > 1000),
            "no capture for {screen}\n{err}"
        );
    }
}
