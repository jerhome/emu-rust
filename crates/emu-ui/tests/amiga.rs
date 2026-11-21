//! Real Amiga tests of the UI layer (3.0, `docs/design/v3.0-amiga.md` §6, Q5b): the real puae and puae2021 cores
//! with an EMPTY system folder (built-in AROS, never a Kickstart), generated disks and Amiga Test Kit.
//!
//! All tests are `#[ignore]`: `scripts/fetch-test-assets.sh`, `cargo build -p emu-app`, then
//! `cargo test -p emu-ui --test amiga -- --ignored --test-threads=1`.
//! Part 1 goes through the same functions as the library launch (`content_spec`, `resolve_game`,
//! `prepare_launch_firmware`, `Emulator::load_content_as`). Part 2 starts the `emu-rust` binary on a private Xvfb
//! and injects real X events (`tests/tools/xinject.py`): Alt left / right must reach the core as the Amiga keys.
//! PNGs land in `target/test-output/ui-amiga/`.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use emu_assets::cores::{CoreId, CorePreferences};
use emu_runtime::{
    CollectSink, Emulator, EmulatorOptions, InputBackend, JoypadButton, NullInput, RetroKey,
    ScriptedInput, reset_core_game_files,
};
use emu_ui::{Config, DataDirs, GameContent, content_spec, prepare_launch_firmware, resolve_game};
use std::sync::atomic::AtomicBool;

/// One libretro core per process (ADR 0008).
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const CORES: [&str; 2] = ["puae", "puae2021"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn core_path(core: &str) -> PathBuf {
    root().join(format!("tests/cores/{core}_libretro.so"))
}

fn out_dir(name: &str) -> PathBuf {
    let dir = root().join("target/test-output/ui-amiga").join(name);
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

/// A data root laid out like the app's; `system/` stays EMPTY (AROS).
fn data_dirs(dir: &Path) -> DataDirs {
    let dirs = DataDirs::under(&dir.join("data"));
    dirs.create_all().unwrap();
    dirs
}

fn emu_options(dirs: &DataDirs, core_variables: Vec<(String, String)>) -> EmulatorOptions {
    EmulatorOptions {
        system_dir: dirs.system.clone(),
        save_dir: dirs.saves.clone(),
        state_dir: dirs.states.clone(),
        assets_dir: dirs.assets.clone(),
        core_variables,
        restore_added_disks: true,
        cache_dir: dirs.cache_dir(),
        provide_vfs: false,
    }
}

struct Launch {
    emu: Emulator,
    dirs: DataDirs,
    /// Output of `prepare_launch_firmware`.
    notice: Option<String>,
    computed: Vec<(String, String)>,
    options_path: PathBuf,
}

/// The library launch of a game on `core`: `GameContent` -> `content_spec` -> `resolve_game` -> firmware check
/// -> runtime. `extra` options are applied last (what a user setting would do).
fn launch_with(core: &str, content: &GameContent, dir: &Path, extra: &[(&str, &str)]) -> Launch {
    require(&core_path(core));
    // puae uses `sinf` without linking libm itself: the host process must have it loaded (emu-app does, through
    // egui). This test binary would not link libm otherwise.
    assert!(std::hint::black_box(0.5f32).sin() > 0.0);
    let dirs = data_dirs(dir);
    let spec = content_spec(core, content, &CorePreferences::builtin());
    let resolved = resolve_game(core, spec, &dirs);
    let setup = prepare_launch_firmware(
        core,
        content,
        &dirs,
        &Config::default(),
        &resolved.options_path,
    );
    let mut variables = setup.computed.clone();
    variables.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    let emu = Emulator::load_content_as(
        &core_path(core),
        resolved.spec,
        resolved.identity,
        emu_options(&dirs, variables),
    )
    .unwrap_or_else(|e| panic!("load {core}: {e}"));
    Launch {
        emu,
        dirs,
        notice: setup.notice,
        computed: setup.computed,
        options_path: resolved.options_path,
    }
}

fn launch(core: &str, content: &GameContent, dir: &Path) -> Launch {
    launch_with(core, content, dir, &[])
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
        .collect::<HashSet<[u8; 4]>>()
        .len()
}

// ---- 1. option names and values the UI assumes -----------------------------------------------------------

/// Dump of the variables of the real cores (SET_VARIABLES / v2 options) and check of every name and value that
/// U4 and `cores.toml` rely on. The full dump lands in `target/test-output/ui-amiga/options-<core>.txt`.
#[test]
#[ignore = "needs puae, puae2021 and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn core_options_match_the_names_assumed_by_the_ui() {
    let _guard = lock();
    for core in CORES {
        let dir = out_dir(&format!("options-{core}"));
        let mut l = launch(core, &GameContent::NoGame, &dir);
        run(&mut l.emu, &mut NullInput, 5);
        let options = l.emu.options();
        let mut dump = String::new();
        for o in &options {
            let values: Vec<&str> = o.values.iter().map(|v| v.value.as_str()).collect();
            dump.push_str(&format!(
                "{} | default={} | current={} | visible={} | {:?}\n",
                o.key, o.default, o.current, o.visible, values
            ));
        }
        fs::write(dir.join("../").join(format!("options-{core}.txt")), &dump).unwrap();
        let by_key: BTreeMap<&str, &emu_runtime::CoreOption> =
            options.iter().map(|o| (o.key.as_str(), o)).collect();
        let values = |key: &str| -> Vec<String> {
            by_key
                .get(key)
                .unwrap_or_else(|| panic!("{core}: option `{key}` does not exist"))
                .values
                .iter()
                .map(|v| v.value.clone())
                .collect()
        };
        // U4 pins these four (ui/mod.rs PINNED_OPTIONS).
        for key in [
            "puae_model",
            "puae_kickstart",
            "puae_video_standard",
            "puae_floppy_multidrive",
        ] {
            assert!(by_key.contains_key(key), "{core}: `{key}` missing");
        }
        // cores.toml frontend options (name and value both).
        for (key, value) in [
            ("puae_mapper_select", "---"),
            ("puae_mapper_mouse_toggle", "---"),
            ("puae_physicalmouse", "enabled"),
            ("puae_floppy_write_redirect", "disabled"),
            ("puae_kickstart", "aros"),
        ] {
            assert!(
                values(key).iter().any(|v| v == value),
                "{core}: `{key}` has no value `{value}`: {:?}",
                values(key)
            );
        }
        // Model values written by the tag table (`MODEL_TAGS`) and listed in `cores.toml` `files`.
        let models = values("puae_model");
        for m in [
            "auto", "A500OG", "A500", "A500PLUS", "A600OG", "A600", "A1200OG", "A1200", "CD32",
            "A4030", "A4040",
        ] {
            assert!(
                models.iter().any(|v| v == m),
                "{core}: puae_model has no `{m}`: {models:?}"
            );
        }
        // Model defaults of launch_firmware.rs: A500 for floppies, A1200 for hard disks.
        for key in ["puae_model_fd", "puae_model_hd"] {
            let v = values(key);
            let want = if key.ends_with("fd") { "A500" } else { "A1200" };
            assert!(v.iter().any(|x| x == want), "{core}: {key} {v:?}");
        }
        eprintln!(
            "{core}: video_standard={:?} multidrive={:?} kickstart={:?}",
            values("puae_video_standard"),
            values("puae_floppy_multidrive"),
            values("puae_kickstart")
        );
    }
}

fn key(name: &str) -> RetroKey {
    RetroKey::from_name(name).unwrap_or_else(|| panic!("no RetroKey `{name}`"))
}

/// Pixels of the frame in `x..x + w`, `y..y + h` for which `pred(r, g, b)` holds.
fn count_in(emu: &Emulator, rect: (u32, u32, u32, u32), pred: impl Fn(u8, u8, u8) -> bool) -> u32 {
    let f = emu.frame();
    let (x0, y0, w, h) = rect;
    assert!(
        x0 + w <= f.width() && y0 + h <= f.height(),
        "frame {}x{} too small for {rect:?}",
        f.width(),
        f.height()
    );
    let mut n = 0;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let i = ((y * f.width() + x) * 4) as usize;
            let px = &f.pixels()[i..i + 3];
            n += u32::from(pred(px[0], px[1], px[2]));
        }
    }
    n
}

/// Keys of the Amiga Test Kit keyboard screen, in the 720x287 frame (x, y, w, h): a pressed key is drawn
/// light grey on the dark purple background.
const KEY_ALT_LEFT: (u32, u32, u32, u32) = (95, 142, 20, 8);
const KEY_AMIGA_LEFT: (u32, u32, u32, u32) = (128, 142, 20, 8);
const KEY_AMIGA_RIGHT: (u32, u32, u32, u32) = (380, 142, 12, 8);
const KEY_ALT_RIGHT: (u32, u32, u32, u32) = (408, 142, 20, 8);

fn key_lit(emu: &Emulator, rect: (u32, u32, u32, u32)) -> bool {
    let total = rect.2 * rect.3;
    // Light grey while changing, green once steady.
    let on = |r: u8, g: u8, b: u8| {
        (r > 150 && g > 150 && b > 150)
            || (g > 150 && i32::from(g) > i32::from(r) + 60 && i32::from(g) > i32::from(b) + 60)
    };
    count_in(emu, rect, on) > total / 2
}

/// Which of [Alt left, Amiga left, Amiga right, Alt right] are drawn pressed.
fn amiga_modifier_keys(emu: &Emulator) -> [bool; 4] {
    [KEY_ALT_LEFT, KEY_AMIGA_LEFT, KEY_AMIGA_RIGHT, KEY_ALT_RIGHT].map(|r| key_lit(emu, r))
}

/// Reads `tests/fingerprints/amiga-puae.txt` (written by the runtime test) for a pinned fingerprint.
fn pinned(name: &str) -> Option<u64> {
    let text = fs::read_to_string(root().join("tests/fingerprints/amiga-puae.txt")).ok()?;
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .find(|l| l.starts_with(name))
        .and_then(|l| l.split("fingerprint=").nth(1))
        .and_then(|h| u64::from_str_radix(h.trim(), 16).ok())
}

// ---- 2. AROS without Kickstart ---------------------------------------------------------------------------

/// Empty system folder, both cores: the first launch warns (once per game, remembered), AROS boots `boot.adf` and
/// the rainbow is on screen; the same disk gives the same picture twice and the original is never written.
#[test]
#[ignore = "needs puae, puae2021 and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn aros_boots_a_disk_and_warns_once_on_both_cores() {
    let _guard = lock();
    let adf = boot_adf();
    let before = sha(&adf);
    let content = GameContent::Files(vec![adf.clone()]);
    for core in CORES {
        let dir = out_dir(&format!("aros-{core}"));
        let mut prints = Vec::new();
        for launch_no in 0..2 {
            let mut l = launch(core, &content, &dir);
            assert_eq!(
                l.computed,
                vec![("puae_kickstart".to_owned(), "aros".to_owned())],
                "{core}: AROS is a computed default, never a saved setting"
            );
            if launch_no == 0 {
                let notice = l.notice.clone().expect("warning on the first launch");
                assert!(notice.contains("kick34005.A500"), "{notice}");
                let saved = fs::read_to_string(&l.options_path).unwrap();
                assert!(saved.contains("aros_acknowledged = true"), "{saved}");
                assert!(
                    !saved.contains("puae_kickstart"),
                    "AROS must not be written as a user option:\n{saved}"
                );
            } else {
                assert_eq!(l.notice, None, "{core}: the warning is given once per game");
            }
            assert!(
                l.emu
                    .system_info()
                    .library_name
                    .to_lowercase()
                    .contains("puae"),
                "{core}"
            );
            run(&mut l.emu, &mut NullInput, 900);
            png(&l.emu, &dir, &format!("boot-{launch_no}"));
            // The rainbow: many distinct rows, not a uniform screen.
            assert!(
                colours(&l.emu) >= 4,
                "{core}: only {} colours",
                colours(&l.emu)
            );
            prints.push(l.emu.frame().fingerprint());
        }
        assert_eq!(prints[0], prints[1], "{core}: boot is not reproducible");
        if core == "puae" {
            match pinned("boot-adf") {
                Some(fp) => assert_eq!(
                    prints[0], fp,
                    "differs from the runtime test's pinned boot-adf"
                ),
                None => eprintln!("no pinned boot-adf fingerprint"),
            }
        }
        eprintln!("{core}: boot.adf fingerprint {:x}", prints[0]);
        assert_eq!(sha(&adf), before, "{core}: the original disk was modified");
    }
}

/// « Workbench / AROS » (no content): the AROS screen is the 720x574-ish interlaced « insert a disk » picture,
/// taller than the 287-line rainbow, on both cores.
#[test]
#[ignore = "needs puae and puae2021: scripts/fetch-test-assets.sh"]
fn aros_desktop_without_a_disk_on_both_cores() {
    let _guard = lock();
    for core in CORES {
        let dir = out_dir(&format!("desktop-{core}"));
        let mut l = launch(core, &GameContent::NoGame, &dir);
        run(&mut l.emu, &mut NullInput, 1000);
        png(&l.emu, &dir, "desktop-1000");
        let (w, h) = (l.emu.frame().width(), l.emu.frame().height());
        assert!(w >= 640 && h >= 400, "{core}: frame {w}x{h}");
        assert!(colours(&l.emu) >= 3, "{core}: uniform screen");
        eprintln!("{core}: AROS desktop {w}x{h}, {} colours", colours(&l.emu));
        if core == "puae"
            && let Some(fp) = pinned("aros-no-game")
        {
            assert_eq!(l.emu.frame().fingerprint(), fp, "pinned aros-no-game");
        }
    }
}

/// Amiga Test Kit 1.21 (Unlicense) boots to its menu on both cores, reproducibly, and shows something else than
/// the rainbow.
#[test]
#[ignore = "needs puae, puae2021 and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn amiga_test_kit_boots_on_both_cores() {
    let _guard = lock();
    let atk = atk_adf();
    let before = sha(&atk);
    for core in CORES {
        let dir = out_dir(&format!("atk-{core}"));
        let mut prints = Vec::new();
        for n in 0..2 {
            let mut l = launch(
                core,
                &GameContent::Files(vec![atk.clone()]),
                &dir.join(n.to_string()),
            );
            run(&mut l.emu, &mut NullInput, 900);
            png(&l.emu, &dir, &format!("atk-{n}"));
            assert!(colours(&l.emu) >= 3, "{core}: uniform screen");
            prints.push(l.emu.frame().fingerprint());
        }
        assert_eq!(prints[0], prints[1], "{core}: not reproducible");
        if core == "puae"
            && let Some(fp) = pinned("amiga-test-kit")
        {
            assert_eq!(prints[0], fp, "pinned amiga-test-kit");
        }
        assert_eq!(sha(&atk), before);
    }
}

// ---- 3. input seen by the core -----------------------------------------------------------------------------

/// Amiga Test Kit, F2 (Keyboard): a key the core receives is drawn on its keyboard picture. Alt left / right
/// become the Amiga keys (`lsuper` / `rsuper`, what `KeyRemap` produces: design §3.2) and are seen as
/// « Am » left / right; a plain `lalt` / `ralt` is the Amiga Alt, a different key.
#[test]
#[ignore = "needs puae, puae2021 and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn keyboard_keys_are_seen_by_the_core_on_both_cores() {
    let _guard = lock();
    let atk = atk_adf();
    for core in CORES {
        let dir = out_dir(&format!("keys-{core}"));
        let mut l = launch(core, &GameContent::Files(vec![atk.clone()]), &dir);
        run(&mut l.emu, &mut NullInput, 900);
        let base = l.emu.frame_count();
        // F2 opens the keyboard test; then each phase holds one key for 50 frames and is checked at its end.
        let phases: [(&str, [bool; 4]); 5] = [
            ("lsuper", [false, true, false, false]),
            ("rsuper", [false, false, true, false]),
            ("lalt", [true, false, false, false]),
            ("ralt", [false, false, false, true]),
            ("a", [false, false, false, false]),
        ];
        let mut input = ScriptedInput::new().key(key("f2"), base + 5..base + 15);
        for (i, (name, _)) in phases.iter().enumerate() {
            let start = base + 60 + 60 * i as u64;
            input = input.key(key(name), start..start + 50);
        }
        run(&mut l.emu, &mut input, 55);
        png(&l.emu, &dir, "00-keyboard-screen");
        assert_eq!(l.emu.frame().width(), 720, "{core}");
        assert_eq!(amiga_modifier_keys(&l.emu), [false; 4], "{core}: idle");
        for (i, (name, expected)) in phases.iter().enumerate() {
            run(&mut l.emu, &mut input, if i == 0 { 55 } else { 60 });
            png(&l.emu, &dir, &format!("{:02}-{name}", i + 1));
            assert_eq!(
                amiga_modifier_keys(&l.emu),
                *expected,
                "{core}: holding `{name}`: [Alt L, Amiga L, Amiga R, Alt R]"
            );
        }
        // Both Amiga keys together with a letter (what Alt left + Alt right + A gives).
        let base = l.emu.frame_count();
        let mut input = ScriptedInput::new()
            .key(key("lsuper"), base + 5..base + 60)
            .key(key("rsuper"), base + 5..base + 60)
            .key(key("a"), base + 5..base + 60);
        run(&mut l.emu, &mut input, 50);
        png(&l.emu, &dir, "06-both-amiga-and-a");
        assert_eq!(
            amiga_modifier_keys(&l.emu),
            [false, true, true, false],
            "{core}"
        );
    }
}

/// Amiga Test Kit, F4 (Controller Ports): the mouse moved by the runtime reaches the core's port 1 (« Mouse »,
/// left half of the picture), RetroPad 0 drives the joystick of port 2 (right half). Confirms the port
/// assignment of the design (§3.4: the Amiga is RetroPad port 0, like the ST).
#[test]
#[ignore = "needs puae, puae2021 and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn mouse_and_joystick_reach_their_ports_on_both_cores() {
    let _guard = lock();
    let atk = atk_adf();
    // Cells of the Test Kit's picture (720x287): the « L » button of the mouse (light grey when pressed), then
    // the stick of port 2 (green).
    const MOUSE_LEFT: (u32, u32, u32, u32) = (173, 113, 14, 8);
    const JOY_FIRE: (u32, u32, u32, u32) = (432, 114, 28, 8);
    const JOY_LEFT: (u32, u32, u32, u32) = (460, 143, 20, 8);
    for core in CORES {
        let dir = out_dir(&format!("ports-{core}"));
        let mut l = launch(core, &GameContent::Files(vec![atk.clone()]), &dir);
        run(&mut l.emu, &mut NullInput, 1000);
        let base = l.emu.frame_count();
        let mut input = ScriptedInput::new()
            .key(key("f4"), base + 5..base + 25)
            // Mouse: the left button first (the pointer is still in the empty top-left corner: a click on one
            // of the Test Kit's menu entries would change the screen), then a motion.
            .mouse(0, 0, 1, base + 60..base + 110)
            .mouse(3, 2, 0, base + 120..base + 170)
            // Joystick: RetroPad 0 left + B (fire).
            .press(0, JoypadButton::Left, base + 190..base + 240)
            .press(0, JoypadButton::B, base + 190..base + 240);
        run(&mut l.emu, &mut input, 55);
        png(&l.emu, &dir, "0-ports-idle");
        assert!(
            ![MOUSE_LEFT, JOY_FIRE, JOY_LEFT]
                .iter()
                .any(|r| key_lit(&l.emu, *r)),
            "{core}: idle"
        );
        run(&mut l.emu, &mut input, 45);
        png(&l.emu, &dir, "1-mouse-left-button");
        assert!(
            key_lit(&l.emu, MOUSE_LEFT),
            "{core}: left button not seen on the mouse port"
        );
        assert!(
            !key_lit(&l.emu, JOY_FIRE) && !key_lit(&l.emu, JOY_LEFT),
            "{core}: the mouse button reached the joystick"
        );
        let button = l.emu.frame().fingerprint();
        run(&mut l.emu, &mut input, 65);
        png(&l.emu, &dir, "2-mouse-moved");
        assert_ne!(
            l.emu.frame().fingerprint(),
            button,
            "{core}: the mouse motion was not seen"
        );
        run(&mut l.emu, &mut input, 70);
        png(&l.emu, &dir, "3-joystick");
        assert!(
            key_lit(&l.emu, JOY_FIRE) && key_lit(&l.emu, JOY_LEFT),
            "{core}: RetroPad 0 left + B not seen on the joystick port"
        );
    }
}

// ---- 4. two drives and the disk change -------------------------------------------------------------------

/// Two disks launched through the library path: working copies of both (originals untouched), the generated
/// `.m3u`, DF0 then DF1 with `puae_floppy_multidrive` (Test Kit F3 « Floppy Drive » lists the drives: DF1 exists
/// only with two disks and MultiDrive on), `insert_next` swaps DF0 and wraps.
#[test]
#[ignore = "needs puae, puae2021 and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn two_disks_df0_df1_and_disk_change_on_both_cores() {
    let _guard = lock();
    let atk = atk_adf();
    for core in CORES {
        let dir = out_dir(&format!("two-{core}"));
        let blank = blank_adf(&dir, "SECOND");
        let (h0, h1) = (sha(&atk), sha(&blank));
        let mut drives = Vec::new();
        for (tag, files, multidrive) in [
            (
                "two-multidrive",
                vec![atk.clone(), blank.clone()],
                "enabled",
            ),
            (
                "two-single-drive",
                vec![atk.clone(), blank.clone()],
                "disabled",
            ),
            ("one-disk", vec![atk.clone()], "enabled"),
        ] {
            let mut l = launch_with(
                core,
                &GameContent::Files(files.clone()),
                &dir.join(tag),
                &[("puae_floppy_multidrive", multidrive)],
            );
            let work = l.emu.work_dir().expect("working copies");
            let prepared = l.emu.prepared_media().expect("prepared media").clone();
            assert_eq!(prepared.images.len(), files.len(), "{core} {tag}");
            assert!(
                prepared
                    .images
                    .iter()
                    .all(|p| p.starts_with(&work) && p.is_file()),
                "{core} {tag}: the core must only get copies"
            );
            run(&mut l.emu, &mut NullInput, 1000);
            let base = l.emu.frame_count();
            let mut input = ScriptedInput::new().key(key("f3"), base + 5..base + 25);
            run(&mut l.emu, &mut input, 100);
            png(&l.emu, &dir, &format!("floppy-{tag}"));
            drives.push((tag, l.emu.frame().fingerprint()));
            if tag == "two-multidrive" {
                let mut disks = l.emu.disks().expect("disk control");
                assert_eq!(disks.slots().len(), 2, "{core}");
                assert_eq!(disks.current(), Some(0), "{core}: DF0 holds the first disk");
                assert_eq!(disks.insert_next().unwrap(), 1, "{core}");
                assert_eq!(disks.current(), Some(1), "{core}");
                assert_eq!(disks.insert_next().unwrap(), 0, "{core}: wraps");
            }
        }
        eprintln!("{core}: Floppy IDs screens {drives:x?}");
        // DF1 exists only in the first variant: its picture differs from the other two.
        assert_ne!(
            drives[0].1, drives[1].1,
            "{core}: MultiDrive off must remove DF1"
        );
        assert_ne!(
            drives[0].1, drives[2].1,
            "{core}: a second disk must add DF1"
        );
        assert_eq!(
            (sha(&atk), sha(&blank)),
            (h0, h1),
            "{core}: originals modified"
        );
    }
}

// ---- 5. « Réinitialiser » ---------------------------------------------------------------------------------

/// The core writes `<game>.save<N>.adf` into its saves folder when the save-disk hotkey is used; the menu's
/// « Réinitialiser les disquettes » deletes the working copies and exactly those files (patterns of
/// `cores.toml`), never the other games' or the original.
#[test]
#[ignore = "needs puae, puae2021 and the Amiga test disks: scripts/fetch-test-assets.sh"]
fn reset_removes_the_save_disks_of_the_core_only() {
    let _guard = lock();
    let adf = boot_adf();
    for core in CORES {
        let dir = out_dir(&format!("reset-{core}"));
        let before = sha(&adf);
        let mut l = launch_with(
            core,
            &GameContent::Files(vec![adf.clone()]),
            &dir,
            &[("puae_mapper_save_disk_toggle", "RETROK_F11")],
        );
        run(&mut l.emu, &mut NullInput, 300);
        let base = l.emu.frame_count();
        let mut input = ScriptedInput::new().key(key("f11"), base + 5..base + 25);
        run(&mut l.emu, &mut input, 100);
        let saves_core = l.dirs.saves.join(core);
        let content = l.emu.content_name().expect("content name").to_owned();
        let work = l.emu.work_dir().expect("work dir");
        let core_dir = l.emu.identity().core_save_dir.clone();
        assert_eq!(core_dir, saves_core, "first game of that name: bare folder");
        let save = core_dir.join(format!("{content}.save0.adf"));
        // The real core's naming, checked while it runs; it deletes a save disk nobody wrote to when it unloads,
        // so the file is put back (same name) to stand for a save disk with data in it.
        assert!(
            save.is_file(),
            "{core}: the core did not create {}",
            save.display()
        );
        drop(l);
        if !save.exists() {
            fs::write(&save, vec![0u8; 901_120]).unwrap();
        }
        // Files that must survive: other games, the core's other files, a same-prefix name, a folder.
        let keep: Vec<PathBuf> = [
            format!("{content}2.save0.adf"),
            "Other.save0.adf".to_owned(),
            format!("{content}.srm"),
            format!("{content}.save0.adf.bak"),
        ]
        .iter()
        .map(|n| core_dir.join(n))
        .collect();
        for k in &keep {
            fs::write(k, b"keep").unwrap();
        }
        let patterns = CorePreferences::builtin().game_save_patterns(&CoreId::new(core).unwrap());
        assert_eq!(patterns, ["{game}.save*.adf"], "{core}");
        let removed = reset_core_game_files(&core_dir, &content, &patterns).unwrap();
        assert_eq!(removed, std::slice::from_ref(&save), "{core}");
        assert!(!save.exists());
        for k in &keep {
            assert!(k.exists(), "{core}: {} was deleted", k.display());
        }
        assert!(
            emu_runtime::reset_work_dir(&saves_core, &work).unwrap(),
            "{core}"
        );
        assert!(!work.exists());
        assert_eq!(sha(&adf), before, "{core}: original modified");
    }
}

// ---- 6. the binary under Xvfb --------------------------------------------------------------------------

struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    fn start() -> Xvfb {
        // -displayfd needs a pipe fd: let the shell hand fd 3 to a temp file.
        let tmp = std::env::temp_dir().join(format!("emu-ui-amiga-display-{}", std::process::id()));
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

/// Share (0..1) of the pixels brighter than 58 % grey in a rectangle of a PNG, through ImageMagick (already
/// needed by the capture script): the same rule as `key_lit` (> 150 on 255).
fn png_bright(png: &Path, (x, y, w, h): (u32, u32, u32, u32)) -> f64 {
    let out = Command::new("convert")
        .arg(png)
        .args(["-crop", &format!("{w}x{h}+{x}+{y}"), "+repage"])
        .args([
            "-colorspace",
            "Gray",
            "-threshold",
            "58%",
            "-format",
            "%[fx:mean]",
            "info:",
        ])
        .output()
        .expect("convert (ImageMagick)");
    assert!(out.status.success(), "convert failed");
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// The real host path: X key events (XTEST) -> winit -> `KeyRemap` (`amiga.toml`) -> core. Alt left / right must
/// reach puae as the left / right Amiga keys, never as Alt, and the host key (right Ctrl) is not forwarded. The
/// frame is dumped by the app at its last frame, with the keys still held; the Test Kit keyboard screen (F2) shows
/// what the core received.
#[test]
#[ignore = "needs puae, Xvfb, libXtst, ImageMagick and a built emu-rust: scripts/fetch-test-assets.sh"]
fn alt_left_and_right_are_the_amiga_keys_through_the_binary() {
    let _guard = lock();
    let dir = out_dir("alt-binary");
    let dirs = data_dirs(&dir);
    let atk = atk_adf();
    let shot = dir.join("frame.png");
    let x = Xvfb::start();
    let child = app(
        &x.display,
        &[
            os("--core"),
            core_path("puae").as_os_str(),
            os("--rom"),
            atk.as_os_str(),
            os("--data-dir"),
            dirs.root.as_os_str(),
            os("--no-audio"),
            os("--frames"),
            os("2400"),
            os("--dump-frame"),
            shot.as_os_str(),
        ],
    )
    .spawn()
    .expect("spawn emu-rust");
    // Boot of the Test Kit (~900 frames = 18 s), F2 = keyboard screen, then the keys are held to the end.
    std::thread::sleep(Duration::from_secs(24));
    inject(
        &x.display,
        &[
            "focus", "sleep", "0.5", "tap", "F2", "sleep", "1.5", "down", "Alt_L", "down", "Alt_R",
            "down", "a", "sleep", "1",
        ],
    );
    let stderr = wait_exit(child, Duration::from_secs(120));
    drop(x);
    assert!(
        shot.is_file(),
        "no frame dumped\n--- emu-rust stderr ---\n{stderr}"
    );
    let lit = |rect| png_bright(&shot, rect) > 0.5;
    let seen = [
        lit(KEY_ALT_LEFT),
        lit(KEY_AMIGA_LEFT),
        lit(KEY_AMIGA_RIGHT),
        lit(KEY_ALT_RIGHT),
    ];
    assert_eq!(
        seen,
        [false, true, true, false],
        "[Alt L, Amiga L, Amiga R, Alt R] drawn pressed by the Test Kit; see {}\n{stderr}",
        shot.display()
    );
}

// ---- 7. interface captures -------------------------------------------------------------------------------

/// Data root of a capture scene: both Amiga cores installed (copied under their file names), disks under
/// `roms/amiga/`, library database already scanned (a capture never scans), EMPTY `system/`.
fn scene_data(dir: &Path) -> DataDirs {
    let dirs = data_dirs(dir);
    for core in CORES {
        require(&core_path(core));
        fs::copy(
            core_path(core),
            dirs.cores.join(format!("{core}_libretro.so")),
        )
        .unwrap();
    }
    let roms = dirs.root.join("roms/amiga");
    fs::create_dir_all(&roms).unwrap();
    fs::copy(boot_adf(), roms.join("Rainbow Demo (1990)(Test).adf")).unwrap();
    fs::copy(atk_adf(), roms.join("Amiga Test Kit 1.21.adf")).unwrap();
    let (mut library, _) = emu_library::Library::open(&dirs.library_db).expect("library");
    let options = emu_library::ScanOptions {
        roots: vec![dirs.root.join("roms")],
        ..Default::default()
    };
    let report = library
        .scan(&options, &AtomicBool::new(false), &mut |_| {})
        .expect("scan");
    assert_eq!(report.added, 2, "{report:?}");
    dirs
}

fn captures_dir() -> PathBuf {
    out_dir("captures")
}

/// Standard deviation of the grey levels (0 for a uniform image): a blank capture is a failed capture.
fn png_spread(png: &Path) -> f64 {
    let out = Command::new("convert")
        .arg(png)
        .args([
            "-colorspace",
            "Gray",
            "-format",
            "%[fx:standard_deviation]",
            "info:",
        ])
        .output()
        .expect("convert (ImageMagick)");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0)
}

/// Mean absolute difference (0..1) between two captures of the same size.
fn png_diff(a: &Path, b: &Path) -> f64 {
    let out = Command::new("convert")
        .arg(a)
        .arg(b)
        .args([
            "-compose",
            "difference",
            "-composite",
            "-colorspace",
            "Gray",
        ])
        .args(["-format", "%[fx:mean]", "info:"])
        .output()
        .expect("convert (ImageMagick)");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0)
}

/// `--screen <screen> --dump-window`: the app renders a few passes, writes the PNG itself and quits.
fn dump_screen(display: &str, data: &DataDirs, screen: &str, envs: &[(&str, &str)], out: &Path) {
    let mut cmd = app(
        display,
        &[
            os("--data-dir"),
            data.root.as_os_str(),
            os("--no-audio"),
            os("--screen"),
            os(screen),
            os("--dump-window"),
            out.as_os_str(),
        ],
    );
    cmd.envs(envs.iter().copied());
    let err = wait_exit(
        cmd.spawn().expect("spawn emu-rust"),
        Duration::from_secs(90),
    );
    assert!(
        fs::metadata(out).is_ok_and(|m| m.len() > 1000),
        "no capture for {screen}\n{err}"
    );
}

/// The interactive library / cores window, one scripted click sequence (coordinates of the 1280x720 window at
/// the top left of the Xvfb screen), then an `xwd` capture of the X root.
fn click_scene(
    display: &str,
    data: &DataDirs,
    screen: &str,
    envs: &[(&str, &str)],
    actions: &[&str],
    out: &Path,
) {
    let mut cmd = app(
        display,
        &[
            os("--data-dir"),
            data.root.as_os_str(),
            os("--no-audio"),
            os("--screen"),
            os(screen),
        ],
    );
    cmd.envs(envs.iter().copied());
    let mut child = cmd.spawn().expect("spawn emu-rust");
    std::thread::sleep(Duration::from_secs(4));
    let mut all = vec!["focus", "sleep", "0.5"];
    all.extend_from_slice(actions);
    all.extend_from_slice(&["sleep", "1.5"]);
    inject(display, &all);
    let xwd = Command::new("xwd")
        .args(["-root", "-silent", "-display", display])
        .output()
        .expect("xwd");
    assert!(xwd.status.success(), "xwd failed");
    let mut convert = Command::new("convert")
        .args(["xwd:-", &format!("png:{}", out.display())])
        .stdin(Stdio::piped())
        .spawn()
        .expect("convert");
    use std::io::Write;
    convert
        .stdin
        .take()
        .unwrap()
        .write_all(&xwd.stdout)
        .unwrap();
    assert!(convert.wait().unwrap().success());
    let _ = child.kill();
    let _ = child.wait();
    assert!(png_spread(out) > 0.02, "blank capture {}", out.display());
}

/// Captures of the 3.0 interface (PNGs kept in `target/test-output/ui-amiga/captures/` for review): the AROS
/// warning dialog, the Cores screen with the puae2021 alternative « recommandé sur Mac », the Kickstart import
/// in the built-in file browser (container mode) and the ROM folder button in a container.
#[test]
#[ignore = "needs puae, puae2021, Xvfb, libXtst, ImageMagick and a built emu-rust: scripts/fetch-test-assets.sh"]
fn interface_captures() {
    let _guard = lock();
    let out = captures_dir();
    let data = scene_data(&out.join("scene"));
    let x = Xvfb::start();
    let d = x.display.as_str();

    // AROS warning: the first game of the library is launched with an empty system folder.
    let aros = out.join("1-aros-dialog.png");
    dump_screen(d, &data, "library-launch", &[], &aros);
    let library = out.join("0-library.png");
    dump_screen(d, &data, "library", &[], &library);
    assert!(
        png_diff(&aros, &library) > 0.002,
        "the AROS dialog is not drawn over the library"
    );

    // Cores screen (static), then the « Core utilisé par système » selector opened by a click.
    let cores = out.join("2-cores.png");
    dump_screen(d, &data, "cores", &[], &cores);
    let selector = out.join("3-cores-selector-puae2021-mac.png");
    click_scene(
        d,
        &data,
        "cores",
        &[],
        &["move", "95", "227", "click", "1"],
        &selector,
    );
    assert!(
        png_diff(&cores, &selector) > 0.002,
        "the selector did not open"
    );

    // Kickstart import: « Importer un BIOS… », built-in browser (EMU_RUST_CONTAINER), a copy of a ROM that is
    // not a known Kickstart (generated zeros: never a real ROM).
    let home = out.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("kick13-copie.rom"), vec![0u8; 262_144]).unwrap();
    let envs = [
        ("EMU_RUST_CONTAINER", "1"),
        ("HOME", home.to_str().unwrap()),
    ];
    let browser = out.join("4-import-browser.png");
    click_scene(
        d,
        &data,
        "cores",
        &envs,
        &["move", "290", "68", "click", "1"],
        &browser,
    );
    assert!(
        png_diff(&cores, &browser) > 0.002,
        "the browser did not open"
    );
    let unknown = out.join("5-import-unknown-file.png");
    click_scene(
        d,
        &data,
        "cores",
        &envs,
        &[
            "move", "290", "68", "click", "1", "sleep", "1", "move", "360", "386", "click", "1",
        ],
        &unknown,
    );
    assert!(
        png_diff(&browser, &unknown) > 0.002,
        "the file was not examined"
    );
    assert!(
        fs::read_dir(&data.system).unwrap().next().is_none(),
        "nothing may be imported by merely looking at an unknown file"
    );

    // ROM folder button in a container: the path (and the host hint) is shown, nothing is opened.
    let folder = out.join("6-rom-folder-container.png");
    click_scene(
        d,
        &data,
        "library",
        &[
            ("EMU_RUST_CONTAINER", "1"),
            ("EMU_HOST_ROMS_DIR", "C:\\Users\\moi\\roms"),
        ],
        &["move", "801", "16", "click", "1"],
        &folder,
    );
    assert!(png_diff(&library, &folder) > 0.002, "no folder dialog");

    // Amiga virtual keyboard over the AROS « insert a disk » screen (no content: « Workbench / AROS »).
    let vkbd = out.join("7-amiga-virtual-keyboard.png");
    let child = app(
        d,
        &[
            os("--core"),
            core_path("puae").as_os_str(),
            os("--no-game"),
            os("--data-dir"),
            data.root.as_os_str(),
            os("--no-audio"),
            os("--frames"),
            os("1100"),
            os("--screen"),
            os("vkbd"),
            os("--dump-window"),
            vkbd.as_os_str(),
        ],
    )
    .spawn()
    .expect("spawn emu-rust");
    let err = wait_exit(child, Duration::from_secs(120));
    assert!(
        fs::metadata(&vkbd).is_ok_and(|m| m.len() > 1000),
        "no virtual keyboard capture\n{err}"
    );
}
