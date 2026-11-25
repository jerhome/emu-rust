//! Real Mega CD tests (2.1 express, `docs/design/v2.1-megacd-express.md` §5): genesis_plus_gx, generated discs
//! (`tests/tools/mkcd.py`) and, when fetched, 240p Test Suite Sega CD (GPL-2.0).
//!
//! All `#[ignore]`: `scripts/fetch-test-assets.sh`, `cargo build -p emu-app`, then
//! `cargo test -p emu-ui --test megacd -- --ignored --test-threads=1`.
//! No commercial BIOS is ever shipped or downloaded: without one, the core must refuse the disc and name the BIOS
//! file it wants. `EMU_TEST_MEGACD_BIOS=<dir>` (the user's own `bios_CD_*.bin`) enables the real boot test.
//! The generic "BIOS manquant" dialog (6.0) lives in the library window (no scripted path from here): it is
//! covered by the unit tests of `emu-ui/src/library`; the core's error shown on a refusal by `core_error_ui.rs`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use emu_assets::cores::{CorePreferences, KnownFirmware};
use emu_library::{DiscRegion, disc_info};
use emu_runtime::{CollectSink, ContentSpec, Emulator, EmulatorOptions, NullInput};
use emu_ui::{GameContent, content_spec};

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn core_path() -> PathBuf {
    root().join("tests/cores/genesis_plus_gx_libretro.so")
}

fn out_dir(name: &str) -> PathBuf {
    let dir = root().join("target/test-output/ui-megacd").join(name);
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

/// The rule the core applies (design C2), restated here on purpose: the probe must agree with the real core.
fn bios_for(region: Option<DiscRegion>) -> &'static str {
    match region {
        Some(DiscRegion::Europe) => "bios_CD_E.bin",
        Some(DiscRegion::Japan) => "bios_CD_J.bin",
        Some(DiscRegion::Usa) | None => "bios_CD_U.bin",
    }
}

fn mkcd(args: &[&str]) {
    let out = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools/mkcd.py"))
        .args(args)
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "mkcd.py: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// FNV-1a of every file under `dir` (relative path -> hash), to prove nothing was touched.
fn tree_hash(dir: &Path) -> BTreeMap<String, u64> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, u64>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let h = fs::read(&path)
                    .unwrap()
                    .iter()
                    .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
                    });
                let rel = path.strip_prefix(base).unwrap();
                out.insert(rel.to_string_lossy().into_owned(), h);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// Files under `dir`, ignoring the game-identity registry the runtime keeps (`.emu-rust-games` and its lock) and
/// the 7.1 log folder.
fn files_under(dir: &Path) -> Vec<String> {
    tree_hash(dir)
        .into_keys()
        .filter(|f| {
            let name = f.rsplit(['/', '\\']).next().unwrap_or(f);
            // `logs/` (7.1, ADR 0023 §3): the application journal is written by every launch, even a refused one.
            name != ".emu-rust-games" && name != ".emu-rust-games.lock" && !f.starts_with("logs/")
        })
        .collect()
}

/// A data root laid out like the app's, with an empty `system/`.
fn data_root(dir: &Path) -> PathBuf {
    let data = dir.join("data");
    for sub in ["system", "saves", "states", "assets"] {
        fs::create_dir_all(data.join(sub)).unwrap();
    }
    data
}

fn emu_options(data: &Path, variables: Vec<(String, String)>) -> EmulatorOptions {
    EmulatorOptions {
        system_dir: data.join("system"),
        save_dir: data.join("saves"),
        cache_dir: data.join("cache"),
        provide_vfs: false,
        state_dir: data.join("states"),
        assets_dir: data.join("assets"),
        core_variables: variables,
        restore_added_disks: false,
        core_save_scope: emu_runtime::SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
        port_devices: Vec::new(),
    }
}

// ---- the binary under Xvfb ------------------------------------------------------------------------------

struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    fn start() -> Xvfb {
        let tmp = std::env::temp_dir().join(format!("emu-ui-cd-display-{}", std::process::id()));
        let _ = fs::remove_file(&tmp);
        let child = Command::new("bash")
            .arg("-c")
            .arg(format!(
                "exec Xvfb -displayfd 3 -screen 0 1280x800x24 -nolisten tcp 3>{} 2>/dev/null",
                tmp.display()
            ))
            .spawn()
            .expect("Xvfb is needed for these tests");
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

/// Runs `emu-rust --core genesis_plus_gx --rom disc` to completion; returns (success, stderr).
fn run_app(x: &Xvfb, disc: &Path, data: &Path) -> (bool, String) {
    let mut child = Command::new(app_binary())
        .env_remove("WAYLAND_DISPLAY")
        .env("DISPLAY", &x.display)
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .args(["--core"])
        .arg(core_path())
        .arg("--rom")
        .arg(disc)
        .arg("--data-dir")
        .arg(data)
        .args(["--no-audio", "--frames", "5"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn emu-rust");
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("emu-rust did not exit within 60 s");
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let mut err = String::new();
    if let Some(mut s) = child.stderr.take() {
        use std::io::Read;
        let _ = s.read_to_string(&mut err);
    }
    (status.success(), err)
}

// ---- tests ----------------------------------------------------------------------------------------------

/// Without any BIOS the core refuses the disc and names the file the probe predicted, for every region, for a
/// `.cue` + raw track and for an `.iso`; the disc folder and the data root stay untouched (nothing written).
#[test]
#[ignore = "needs genesis_plus_gx, Xvfb and a built emu-rust: scripts/fetch-test-assets.sh"]
fn core_asks_for_the_bios_the_probe_predicts_and_writes_nothing() {
    let _guard = lock();
    require(&core_path());
    let dir = out_dir("probe");
    let x = Xvfb::start();
    for (code, region) in [
        ("E", DiscRegion::Europe),
        ("J", DiscRegion::Japan),
        ("U", DiscRegion::Usa),
    ] {
        let disc_dir = dir.join(format!("disc{code}"));
        mkcd(&[
            "megacd",
            disc_dir.to_str().unwrap(),
            "--name",
            "Probe",
            "--region",
            code,
            "--iso",
        ]);
        for file in ["Probe.cue", "Probe.iso"] {
            let disc = disc_dir.join(file);
            let info = disc_info(&disc).unwrap().expect("Sega CD header");
            assert_eq!(info.region, Some(region), "{file} probe");
            let wanted = bios_for(info.region);

            let before = tree_hash(&disc_dir);
            let data = data_root(&dir.join(format!("run-{code}-{file}")));
            let data_before = files_under(&data);
            let (ok, stderr) = run_app(&x, &disc, &data);
            assert!(!ok, "no BIOS: the launch must fail ({file}, {code})");
            assert!(
                stderr.contains("Unable to open CD BIOS") && stderr.contains(wanted),
                "{file} ({code}) expected a request for {wanted}, got:\n{stderr}"
            );
            for other in ["bios_CD_E.bin", "bios_CD_J.bin", "bios_CD_U.bin"] {
                if other != wanted {
                    assert!(
                        !stderr.contains(&format!("{other}\"")),
                        "{file} ({code}) the core asked for {other} too:\n{stderr}"
                    );
                }
            }
            assert_eq!(tree_hash(&disc_dir), before, "disc folder modified");
            assert_eq!(files_under(&data), data_before, "files written in {data:?}");
        }
    }
}

/// Same agreement on the real 240p Test Suite Sega CD disc (USA), and a failed load through the runtime API
/// (`ContentSpec::File`, what the library launch uses for `megacd`) leaves the disc and `saves/` untouched.
#[test]
#[ignore = "needs genesis_plus_gx and 240p Sega CD USA: scripts/fetch-test-assets.sh"]
fn real_240p_disc_without_bios_fails_cleanly() {
    let _guard = lock();
    require(&core_path());
    let disc_dir = root().join("tests/roms/240pSuite-SegaCD-1.21-USA");
    let cue = disc_dir.join("240pSuite_USA.cue");
    require(&cue);
    assert_eq!(
        disc_info(&cue).unwrap().and_then(|i| i.region),
        Some(DiscRegion::Usa)
    );
    let before = tree_hash(&disc_dir);
    let dir = out_dir("240p");
    let data = data_root(&dir);

    // What the library launch builds for a `megacd` game: the file, in place.
    let spec = content_spec(
        "genesis_plus_gx",
        &GameContent::Files(vec![cue.clone()]),
        &CorePreferences::builtin(),
    );
    assert_eq!(spec, ContentSpec::File(cue.clone()));
    let result = Emulator::load_content(&core_path(), spec, emu_options(&data, vec![]));
    assert!(result.is_err(), "no BIOS: the core must refuse the disc");
    assert_eq!(tree_hash(&disc_dir), before, "disc folder modified");
    assert!(
        files_under(&data.join("saves")).is_empty(),
        "saves written: {:?}",
        files_under(&data.join("saves"))
    );

    let x = Xvfb::start();
    let (ok, stderr) = run_app(&x, &cue, &data);
    assert!(!ok);
    assert!(
        stderr.contains("Unable to open CD BIOS") && stderr.contains("bios_CD_U.bin"),
        "{stderr}"
    );
    assert_eq!(tree_hash(&disc_dir), before, "disc folder modified");
}

/// The three BIOS names are known to the firmware catalogue (md5 from libretro `System.dat`).
#[test]
fn cd_bios_hashes_are_in_the_catalogue() {
    let known = KnownFirmware::builtin();
    for (name, md5) in [
        ("bios_CD_E.bin", "e66fa1dc5820d254611fdcdba0662372"),
        ("bios_CD_J.bin", "278a9397d192149e84e820ac621a8edd"),
        ("bios_CD_U.bin", "2efd74e3232ff260e371b99f84024f7f"),
    ] {
        let hashes = known.md5_for(name);
        let expected: Vec<u8> = (0..16)
            .map(|i| u8::from_str_radix(&md5[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        assert!(
            hashes.iter().any(|h| h[..] == expected[..]),
            "{name}: md5 {md5} not in the catalogue ({} known)",
            hashes.len()
        );
    }
}

/// Local only: the user's own BIOS in `EMU_TEST_MEGACD_BIOS` (copied to a temp system dir, never into the repo).
/// 240p USA must boot (600 frames, picture not black); closing the game writes the per-game backup RAM.
#[test]
#[ignore = "needs genesis_plus_gx, 240p Sega CD USA and EMU_TEST_MEGACD_BIOS=<dir with bios_CD_U.bin>"]
fn real_boot_with_the_users_bios() {
    let _guard = lock();
    let Some(bios_dir) = std::env::var_os("EMU_TEST_MEGACD_BIOS").map(PathBuf::from) else {
        eprintln!("EMU_TEST_MEGACD_BIOS not set: skipped");
        return;
    };
    require(&core_path());
    let cue = root().join("tests/roms/240pSuite-SegaCD-1.21-USA/240pSuite_USA.cue");
    require(&cue);
    let bios = bios_dir.join("bios_CD_U.bin");
    require(&bios);
    let dir = out_dir("boot");
    let data = data_root(&dir);
    fs::copy(&bios, data.join("system/bios_CD_U.bin")).unwrap();

    let variables = vec![(
        "genesis_plus_gx_system_bram".to_owned(),
        "per game".to_owned(),
    )];
    let mut emu = Emulator::load_content(
        &core_path(),
        ContentSpec::File(cue),
        emu_options(&data, variables),
    )
    .expect("load 240p with the user's BIOS");
    let mut audio = CollectSink::default();
    for _ in 0..600 {
        emu.run_frame(&mut NullInput, &mut audio)
            .expect("run_frame");
    }
    emu.frame().write_png(&dir.join("boot.png")).unwrap();
    let colours = emu
        .frame()
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .collect::<std::collections::HashSet<[u8; 4]>>()
        .len();
    assert!(colours > 1, "black or uniform picture");
    drop(emu);
    let saves = files_under(&data.join("saves"));
    assert!(
        saves.iter().any(|f| f.ends_with("240pSuite_USA.brm")),
        "per-game backup RAM missing: {saves:?}"
    );
    assert!(
        !saves.iter().any(|f| f.ends_with("scd_U.brm")),
        "shared per-BIOS RAM written: {saves:?}"
    );
}
