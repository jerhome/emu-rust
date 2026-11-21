//! Real PlayStation and 3DO tests (7.0): pcsx_rearmed, swanstation, mednafen_psx and opera headless, with no BIOS
//! (OpenBIOS / HLE / Opera Boot built into the cores) on free data: 240p Test Suite PS1, ps1-tests, and a fake 3DO
//! disc from `tests/tools/mkcd.py`. Design `docs/design/v7.0-ps1-3do.md` §5.
//!
//! All tests are `#[ignore]`: run `scripts/fetch-test-assets.sh --with-7.0` then
//! `cargo test -p emu-runtime --test psx_3do_real -- --ignored --test-threads=1`. Set `EMU_UPDATE_FINGERPRINTS=1`
//! to (re)write `tests/fingerprints/psx-*.txt` / `3do-opera.txt` after inspecting the PNGs in
//! `target/test-output/psx-3do/`. A missing core or data file fails with the command to run.

#![cfg(target_os = "linux")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use emu_runtime::{
    CollectSink, ContentSpec, Emulator, EmulatorOptions, LibretroError, MemoryKind, NullInput,
    RuntimeError, SaveScope,
};

const PS1_CORES: [&str; 3] = ["pcsx_rearmed", "swanstation", "mednafen_psx"];
/// DualShock device id declared by each PS1 core (design §3.2).
const fn dualshock_id(core: &str) -> u32 {
    if matches!(core.as_bytes(), b"swanstation") {
        261
    } else {
        517
    }
}

/// One libretro core per process at a time (ADR 0008): tests must not overlap.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn core_path(core: &str) -> PathBuf {
    root().join(format!("tests/cores/{core}_libretro.so"))
}

fn rom(rel: &str) -> PathBuf {
    root().join("tests/roms").join(rel)
}

fn out_dir(name: &str) -> PathBuf {
    let dir = root().join("target/test-output/psx-3do").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create output dir");
    dir
}

fn require(path: &Path) -> &Path {
    assert!(
        path.is_file(),
        "missing test asset {}: lancer scripts/fetch-test-assets.sh --with-7.0",
        path.display()
    );
    path
}

fn options(
    data: &Path,
    variables: &[(&str, &str)],
    port_devices: Vec<(u32, u32)>,
) -> EmulatorOptions {
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
        core_variables: variables
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        restore_added_disks: false,
        core_save_scope: SaveScope::Core,
        fastforward_override: false,
        variable_state_size: false,
        port_devices,
    }
}

fn load_spec(
    core: &str,
    spec: ContentSpec,
    data: &Path,
    variables: &[(&str, &str)],
) -> Result<Emulator, RuntimeError> {
    require(&core_path(core));
    Emulator::load_content(&core_path(core), spec, options(data, variables, Vec::new()))
}

fn load_file(core: &str, content: &Path, data: &Path) -> Result<Emulator, RuntimeError> {
    require(content);
    load_spec(core, ContentSpec::File(content.to_path_buf()), data, &[])
}

fn run_frames(emu: &mut Emulator, frames: u64) {
    let mut sink = CollectSink::default();
    for _ in 0..frames {
        emu.run_frame(&mut NullInput, &mut sink).expect("run_frame");
    }
}

/// More than one distinct pixel value on the screen.
fn not_blank(emu: &Emulator) -> bool {
    let px = emu.frame().pixels();
    px.as_chunks::<4>().0.iter().any(|p| p[..] != px[..4])
}

// ---- SET_HW_RENDER requests (logged by emu-libretro as "unhandled command" at DEBUG) ------------------------

static UNHANDLED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

struct CmdVisit(Option<u64>);

impl tracing::field::Visit for CmdVisit {
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "cmd" {
            self.0 = Some(value);
        }
    }
    fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
}

struct Spy;

impl tracing::Subscriber for Spy {
    fn enabled(&self, m: &tracing::Metadata<'_>) -> bool {
        *m.level() <= tracing::Level::DEBUG
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        let mut v = CmdVisit(None);
        e.record(&mut v);
        if let Some(cmd) = v.0 {
            UNHANDLED.lock().unwrap().push(cmd);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn install_spy() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        tracing::subscriber::set_global_default(Spy).expect("no other global subscriber");
    });
}

// ---- fingerprints (same name-merged format as `gba_32x_megacd_real.rs`) -------------------------------------

fn core_hash(core: &str) -> String {
    fs::read_to_string(root().join(format!("tests/cores/{core}_libretro.so.sha256")))
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

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

fn check_fingerprint(file: &str, core: &str, name: &str, frame: u64, fp: u64) {
    let path = root().join("tests/fingerprints").join(file);
    let mut known = fs::read_to_string(&path)
        .map(|t| parse_prints(&t))
        .unwrap_or_default();
    if std::env::var_os("EMU_UPDATE_FINGERPRINTS").is_some() {
        known.retain(|(n, _, _)| n != name);
        known.push((name.to_owned(), frame, fp));
        known.sort();
        let mut out = format!(
            "# {core}, free test data, no BIOS (built into the core); core sha256 {}\n",
            core_hash(core)
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
                "no fingerprint `{name}` in {}: inspect target/test-output/psx-3do/*/*.png then run with EMU_UPDATE_FINGERPRINTS=1",
                path.display()
            )
        });
    assert_eq!(
        (want.1, want.2),
        (frame, fp),
        "fingerprint `{name}` differs; regenerate with EMU_UPDATE_FINGERPRINTS=1 if the change is legitimate"
    );
}

/// One session: `(width, height, fingerprint)` after `frames` frames; writes a PNG.
fn session(core: &str, spec: ContentSpec, dir: &Path, frames: u64) -> (u32, u32, u64) {
    let mut emu = load_spec(core, spec, &dir.join("data"), &[]).expect("load");
    run_frames(&mut emu, frames);
    assert!(
        not_blank(&emu),
        "{core}: blank screen after {frames} frames"
    );
    emu.frame().write_png(&dir.join("last.png")).unwrap();
    (
        emu.frame().width(),
        emu.frame().height(),
        emu.frame().fingerprint(),
    )
}

// ---- PlayStation ---------------------------------------------------------------------------------------------

/// swanstation needs about 1000 frames to leave its BIOS animation.
const BOOT_FRAMES: u64 = 1500;

const SUITE: &str = "psx/240pTestSuitePS1-EMU.cue";

/// 240p Test Suite menu on each core, empty system folder: boots, non-black image, run twice = same image.
/// The fingerprint is pinned per core only when the core is deterministic (checked on two runs).
#[test]
#[ignore = "needs the PS1 cores and 240p Test Suite PS1: scripts/fetch-test-assets.sh --with-7.0"]
fn ps1_cores_boot_240p_without_bios() {
    let _g = lock();
    for core in PS1_CORES {
        let spec = || ContentSpec::File(require(&rom(SUITE)).to_path_buf());
        let a = session(
            core,
            spec(),
            &out_dir(&format!("240p-{core}-a")),
            BOOT_FRAMES,
        );
        let b = session(
            core,
            spec(),
            &out_dir(&format!("240p-{core}-b")),
            BOOT_FRAMES,
        );
        assert!(a.0 >= 256 && a.1 >= 224, "{core}: odd size {}x{}", a.0, a.1);
        assert_eq!(a, b, "{core}: two runs of the 240p menu must be identical");
        check_fingerprint(
            &format!("psx-{core}.txt"),
            core,
            "240p-menu",
            BOOT_FRAMES,
            a.2,
        );
    }
}

/// A PS-EXE of ps1-tests, if the core accepts it directly: boots, not black. A core that refuses a bare `.exe`
/// is reported and skipped (it is not a defect: the design only asks for it "si le core l'accepte").
#[test]
#[ignore = "needs the PS1 cores and ps1-tests: scripts/fetch-test-assets.sh --with-7.0"]
fn ps1_cores_run_a_ps_exe_when_accepted() {
    let _g = lock();
    let mut ran = 0;
    for core in PS1_CORES {
        let exe = require(&rom("psx/ps1-tests-triangle.exe")).to_path_buf();
        match load_file(
            core,
            &exe,
            &out_dir(&format!("triangle-{core}")).join("data"),
        ) {
            Ok(mut emu) => {
                run_frames(&mut emu, 300);
                assert!(not_blank(&emu), "{core}: triangle.exe left a blank screen");
                emu.frame()
                    .write_png(&root().join(format!(
                        "target/test-output/psx-3do/triangle-{core}/last.png"
                    )))
                    .unwrap();
                eprintln!(
                    "OK {core}: triangle.exe, fingerprint {:016x}",
                    emu.frame().fingerprint()
                );
                ran += 1;
            }
            Err(e) => eprintln!("SKIP {core}: refuses a bare PS-EXE ({e})"),
        }
    }
    assert!(ran > 0, "no PS1 core accepted triangle.exe");
}

/// Design §3.2: every PS1 core declares its DualShock id on both ports, and accepts it.
#[test]
#[ignore = "needs the PS1 cores and 240p Test Suite PS1: scripts/fetch-test-assets.sh --with-7.0"]
fn ps1_cores_declare_and_accept_dualshock() {
    let _g = lock();
    for core in PS1_CORES {
        let dir = out_dir(&format!("pad-{core}"));
        let mut emu = load_file(core, &rom(SUITE), &dir).expect("load");
        let id = dualshock_id(core);
        for port in 0..2 {
            let ids: Vec<u32> = emu.controller_types(port).iter().map(|t| t.id).collect();
            assert!(ids.contains(&id), "{core} port {port}: {id} not in {ids:?}");
        }
        emu.set_port_device(0, id).expect("DualShock accepted");
        run_frames(&mut emu, 60);
        assert!(
            matches!(
                emu.set_port_device(0, 0x7777),
                Err(RuntimeError::Libretro(
                    LibretroError::UnknownControllerDevice { .. }
                ))
            ),
            "{core}: undeclared id must be refused"
        );
    }
}

/// `EmulatorOptions::port_devices` is applied at load time.
#[test]
#[ignore = "needs the PS1 cores and 240p Test Suite PS1: scripts/fetch-test-assets.sh --with-7.0"]
fn port_devices_option_is_applied_at_load() {
    let _g = lock();
    for core in PS1_CORES {
        let dir = out_dir(&format!("portopt-{core}"));
        require(&core_path(core));
        let mut emu = Emulator::load_content(
            &core_path(core),
            ContentSpec::File(require(&rom(SUITE)).to_path_buf()),
            options(&dir, &[], vec![(0, dualshock_id(core)), (1, 0x7777)]),
        )
        .expect("an unknown id is logged, never fatal");
        run_frames(&mut emu, 60);
        assert!(not_blank(&emu));
    }
}

/// swanstation with `swanstation_GPU_Renderer = Software` (cores.toml default) never asks for a hardware context.
#[test]
#[ignore = "needs swanstation and 240p Test Suite PS1: scripts/fetch-test-assets.sh --with-7.0"]
fn swanstation_software_renderer_requests_no_hw_context() {
    let _g = lock();
    install_spy();
    UNHANDLED.lock().unwrap().clear();
    let dir = out_dir("swan-sw");
    let mut emu = load_spec(
        "swanstation",
        ContentSpec::File(require(&rom(SUITE)).to_path_buf()),
        &dir,
        &[("swanstation_GPU_Renderer", "Software")],
    )
    .expect("load");
    run_frames(&mut emu, BOOT_FRAMES);
    assert!(not_blank(&emu));
    let seen = UNHANDLED.lock().unwrap().clone();
    assert!(
        !seen.contains(&14),
        "SET_HW_RENDER (14) requested with the Software renderer; unhandled commands: {seen:?}"
    );
}

/// 128 KiB memory card exposed as save RAM; a `.srm` of 128 KiB in the save folder is restored into it. The core
/// formats a blank card itself, so the frontend writes no file until a game saves (`flush_sram` only writes a
/// changed card); 240p Test Suite never saves, hence the seeded file.
#[test]
#[ignore = "needs the PS1 cores and 240p Test Suite PS1: scripts/fetch-test-assets.sh --with-7.0"]
fn ps1_cores_expose_and_restore_a_128_kib_memory_card() {
    let _g = lock();
    for core in PS1_CORES {
        let dir = out_dir(&format!("card-{core}"));
        let srm = {
            let emu = load_file(core, &rom(SUITE), &dir).expect("load");
            let card = emu.memory(MemoryKind::SaveRam).map(<[u8]>::len);
            assert_eq!(
                card,
                Some(128 * 1024),
                "{core}: memory card exposed as save RAM"
            );
            emu.sram_path()
        };
        assert!(
            srm.starts_with(dir.join("saves").join(core)),
            "{}",
            srm.display()
        );
        // A blank, valid card: "MC" header frame (XOR checksum) and a marker deep in the data area.
        let mut seed = vec![0u8; 128 * 1024];
        seed[0] = b'M';
        seed[1] = b'C';
        seed[127] = b'M' ^ b'C';
        for frame in 1..16 {
            seed[frame * 128] = 0xA0;
            seed[frame * 128 + 8..frame * 128 + 10].copy_from_slice(&[0xFF, 0xFF]);
            seed[frame * 128 + 127] = seed[frame * 128..frame * 128 + 127]
                .iter()
                .fold(0, |a, b| a ^ b);
        }
        seed[100 * 1024] = 0x5A;
        fs::create_dir_all(srm.parent().unwrap()).unwrap();
        fs::write(&srm, &seed).unwrap();
        let mut emu = load_file(core, &rom(SUITE), &dir).expect("reload");
        run_frames(&mut emu, 60);
        let card = emu.memory(MemoryKind::SaveRam).expect("card");
        assert_eq!(card.len(), 128 * 1024);
        assert_eq!(
            card[100 * 1024],
            0x5A,
            "{core}: the .srm was not restored into the card"
        );
        drop(emu);
        assert_eq!(
            fs::read(&srm).unwrap().len(),
            128 * 1024,
            "{core}: .srm must stay 128 KiB"
        );
    }
}

/// ADR 0022: two successive loads in the same process, for each PS1 core, give the same image.
#[test]
#[ignore = "needs the PS1 cores and 240p Test Suite PS1: scripts/fetch-test-assets.sh --with-7.0"]
fn ps1_cores_reload_in_the_same_process() {
    let _g = lock();
    for core in PS1_CORES {
        let first = session(
            core,
            ContentSpec::File(rom(SUITE)),
            &out_dir(&format!("reload-{core}-1")),
            BOOT_FRAMES,
        );
        let second = session(
            core,
            ContentSpec::File(rom(SUITE)),
            &out_dir(&format!("reload-{core}-2")),
            BOOT_FRAMES,
        );
        assert_eq!(first, second, "{core}: second session differs");
    }
}

/// Generated bootable disc (`mkcd.py psx --boot`): skipped when the core does not boot it (design §5).
#[test]
#[ignore = "needs the PS1 cores and ps1-tests: scripts/fetch-test-assets.sh --with-7.0"]
fn generated_bootable_disc_starts_or_is_skipped() {
    let _g = lock();
    let exe = require(&rom("psx/ps1-tests-triangle.exe")).to_path_buf();
    let dir = out_dir("mkcd-psx");
    let status = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools/mkcd.py"))
        .args(["psx", "--region", "U", "--boot"])
        .arg(&exe)
        .arg(&dir)
        .status()
        .expect("python3");
    assert!(status.success(), "mkcd.py psx failed");
    let cue = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "cue"))
        .expect("a .cue was generated");
    for core in PS1_CORES {
        let mut emu =
            load_file(core, &cue, &dir.join(format!("data-{core}"))).expect("load generated disc");
        run_frames(&mut emu, 600);
        if not_blank(&emu) {
            eprintln!("OK {core}: generated disc boots");
        } else {
            eprintln!("SKIP {core}: generated disc does not boot (blank)");
        }
    }
}

// ---- 3DO (opera) ---------------------------------------------------------------------------------------------

/// No content, empty system folder: Opera Boot screen, stable, NVRAM-related files do not break the load.
#[test]
#[ignore = "needs opera: scripts/fetch-test-assets.sh --with-7.0"]
fn opera_boots_without_bios_into_opera_boot() {
    let _g = lock();
    let run = |name: &str| {
        let dir = out_dir(name);
        let mut emu = load_spec(
            "opera",
            ContentSpec::NoGame,
            &dir.join("data"),
            &[("opera_random_seed", "0x00000000")],
        )
        .expect("load without content");
        run_frames(&mut emu, 600);
        assert!(not_blank(&emu), "blank screen: Opera Boot expected");
        emu.frame().write_png(&dir.join("last.png")).unwrap();
        let _ = emu.flush_sram();
        (
            emu.frame().width(),
            emu.frame().height(),
            emu.frame().fingerprint(),
        )
    };
    let a = run("opera-boot-a");
    let b = run("opera-boot-b");
    assert_eq!(a, b, "Opera Boot with a fixed seed must be identical");
    check_fingerprint("3do-opera.txt", "opera", "opera-boot", 600, a.2);
}

/// A fake 3DO disc (Opera signature, not bootable) loads without crashing; Opera Boot is displayed anyway.
#[test]
#[ignore = "needs opera: scripts/fetch-test-assets.sh --with-7.0"]
fn opera_loads_a_fake_3do_disc_without_crashing() {
    let _g = lock();
    let dir = out_dir("opera-fake");
    let status = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools/mkcd.py"))
        .arg("3do")
        .arg(&dir)
        .status()
        .expect("python3");
    assert!(status.success(), "mkcd.py 3do failed");
    let cue = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "cue"))
        .expect("a .cue was generated");
    match load_spec(
        "opera",
        ContentSpec::File(cue),
        &dir.join("data"),
        &[("opera_random_seed", "0x00000000")],
    ) {
        Ok(mut emu) => {
            run_frames(&mut emu, 300);
            assert!(not_blank(&emu), "blank screen");
        }
        Err(RuntimeError::Libretro(LibretroError::LoadGameFailed { .. })) => {
            eprintln!("opera refuses the fake disc cleanly (LoadGameFailed): acceptable");
        }
        Err(e) => panic!("unexpected error: {e}"),
    }
}

/// ADR 0022: two successive no-content loads of opera in the same process.
#[test]
#[ignore = "needs opera: scripts/fetch-test-assets.sh --with-7.0"]
fn opera_reloads_in_the_same_process() {
    let _g = lock();
    let one = |name: &str| {
        let mut emu = load_spec(
            "opera",
            ContentSpec::NoGame,
            &out_dir(name).join("data"),
            &[("opera_random_seed", "0x00000000")],
        )
        .expect("load");
        run_frames(&mut emu, 300);
        emu.frame().fingerprint()
    };
    assert_eq!(one("opera-reload-1"), one("opera-reload-2"));
}
