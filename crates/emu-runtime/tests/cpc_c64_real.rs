//! Real Amstrad CPC and C64 tests (5.0): cap32, vice_x64 and vice_x64sc headless, on generated media
//! (`tests/tools/{mkcpc,mkcdt,mkc64,mktap}.py`, public domain, written for this project) and on free third-party
//! homebrews only (`docs/design/v5.0-cpc-c64.md` §6, `docs/design/donnees-test-4-7.md` §5 and §6). No ROM or BIOS:
//! the machine ROMs are built into the cores (user decision of 2026-11-21).
//!
//! All tests are `#[ignore]`: run `scripts/fetch-test-assets.sh --with-5.0` then
//! `cargo test -p emu-runtime --test cpc_c64_real -- --ignored --test-threads=1`. Set `EMU_UPDATE_FINGERPRINTS=1`
//! to (re)write `tests/fingerprints/{cpc-cap32,c64-vice_x64,c64-vice_x64sc}.txt` after inspecting the PNGs in
//! `target/test-output/cpc-c64/`.
//!
//! Fingerprints are exact only for the generated media (deterministic text on the machine's own screen). The
//! third-party homebrews are smoke tests (screen not blank, image changes): nobody has pinned their screens yet.
//! A third-party file that was not downloaded makes its test fail with the fetch hint, like every other asset.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use emu_runtime::{
    CollectSink, ContentSpec, Emulator, EmulatorOptions, InputBackend, M3uLaunch, MAX_COPY_BYTES,
    MediaPolicy, MediaRules, MediaSet, NullInput, RetroKey, SaveScope, ScriptedInput,
};

/// One libretro core per process (ADR 0008): tests must not overlap.
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

fn out_dir(name: &str) -> PathBuf {
    let dir = root().join("target/test-output/cpc-c64").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create output dir");
    dir
}

fn require(path: &Path) -> &Path {
    assert!(
        path.is_file(),
        "missing test asset {}: lancer scripts/fetch-test-assets.sh --with-5.0",
        path.display()
    );
    path
}

fn sha(path: &Path) -> Vec<u8> {
    // FNV-1a is enough to compare a file with itself before and after.
    let h = fs::read(path)
        .unwrap()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        });
    h.to_le_bytes().to_vec()
}

// ---- generated media ---------------------------------------------------------------------------------------

fn python(script: &str, args: &[&Path], extra: &[&str]) {
    let run = Command::new("python3")
        .arg("-I")
        .arg(root().join("tests/tools").join(script))
        .args(args)
        .args(extra)
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .output()
        .expect("python3 is required to run the media generators");
    assert!(
        run.status.success(),
        "{script} failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

/// The CPC and C64 sets of the generators, in `dir/cpc` and `dir/c64` (hello.dsk, two.dsk, hello.cdt, hello.prg,
/// hello.d64, hello.t64, hello.tap, cart.crt, ...).
fn generated(dir: &Path) -> (PathBuf, PathBuf) {
    let (cpc, c64) = (dir.join("cpc"), dir.join("c64"));
    python("mkcpc.py", &[], &["--set", cpc.to_str().unwrap()]);
    python("mkcdt.py", &[], &["--set", cpc.to_str().unwrap()]);
    python("mkc64.py", &[&c64], &["--all"]);
    python(
        "mktap.py",
        &[&c64.join("hello.prg"), &c64.join("hello.tap")],
        &[],
    );
    (cpc, c64)
}

// ---- sessions ----------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Machine {
    Cpc,
    C64,
}

impl Machine {
    fn rules(self) -> MediaRules {
        match self {
            // cores.toml: media = "direct", m3u_launch = "command".
            Machine::Cpc => MediaRules {
                policy: MediaPolicy::Direct,
                media_direct: Vec::new(),
                max_copy_bytes: MAX_COPY_BYTES,
                m3u_launch: M3uLaunch::Command,
                multi_disc: emu_runtime::MultiDisc::Playlist,
            },
            // cores.toml: media = "working_copy", m3u_launch = "program".
            Machine::C64 => MediaRules {
                policy: MediaPolicy::WorkingCopy,
                media_direct: Vec::new(),
                max_copy_bytes: MAX_COPY_BYTES,
                m3u_launch: M3uLaunch::Program,
                multi_disc: emu_runtime::MultiDisc::Playlist,
            },
        }
    }

    /// `options` of the VICE entries of cores.toml (the frontend sends them at load).
    fn core_variables(self) -> Vec<(String, String)> {
        match self {
            Machine::Cpc => Vec::new(),
            Machine::C64 => [
                ("vice_mapper_joyport_switch", "---"),
                ("vice_mapper_reset", "---"),
                ("vice_mapper_select", "---"),
                ("vice_mapper_statusbar", "---"),
                ("vice_datasette_hotkeys", "enabled"),
                ("vice_mapper_datasette_start", "RETROK_F13"),
                ("vice_mapper_datasette_stop", "RETROK_F14"),
                ("vice_mapper_datasette_rewind", "RETROK_F15"),
                ("vice_mapper_datasette_forward", "---"),
                ("vice_mapper_datasette_reset", "---"),
                ("vice_autoloadwarp", "enabled"),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .to_vec(),
        }
    }
}

/// Fresh data directories. cap32: `save_scope = "game"`, `fastforward_override`; VICE: `variable_state_size`.
fn options(data: &Path, machine: Machine) -> EmulatorOptions {
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
        core_variables: machine.core_variables(),
        restore_added_disks: false,
        core_save_scope: if machine == Machine::Cpc {
            SaveScope::Game
        } else {
            SaveScope::Core
        },
        fastforward_override: true,
        variable_state_size: machine == Machine::C64,
        port_devices: Vec::new(),
    }
}

fn media(machine: Machine, name: &str, image: &Path, launch: Option<&str>) -> ContentSpec {
    ContentSpec::Media {
        set: MediaSet {
            name: name.into(),
            images: vec![image.to_path_buf()],
            playlist: None,
            launch: launch.map(str::to_owned),
        },
        rules: machine.rules(),
    }
}

fn playlist(machine: Machine, name: &str, m3u: &Path) -> ContentSpec {
    ContentSpec::Media {
        set: MediaSet {
            name: name.into(),
            images: Vec::new(),
            playlist: Some(m3u.to_path_buf()),
            launch: None,
        },
        rules: machine.rules(),
    }
}

fn load(core: &str, machine: Machine, spec: ContentSpec, data: &Path) -> Emulator {
    require(&core_path(core));
    Emulator::load_content(&core_path(core), spec, options(data, machine)).expect("load core")
}

/// Runs `frames` frames, collecting `(frame, fingerprint)` at each checkpoint and the `fastforward_changed`
/// requests as `(frame, on)`; a PNG is written for each checkpoint.
struct Played {
    prints: Vec<(u64, u64)>,
    ff: Vec<(u64, bool)>,
}

fn play(emu: &mut Emulator, dir: &Path, checkpoints: &[u64]) -> Played {
    let mut sink = CollectSink::default();
    let mut played = Played {
        prints: Vec::new(),
        ff: Vec::new(),
    };
    for n in 1..=*checkpoints.last().unwrap() {
        let status = emu.run_frame(&mut NullInput, &mut sink).expect("run_frame");
        if status.fastforward_changed {
            let on = emu.fastforward_override().is_some_and(|f| f.fastforward);
            played.ff.push((n, on));
        }
        if checkpoints.contains(&n) {
            played.prints.push((n, emu.frame().fingerprint()));
            emu.frame()
                .write_png(&dir.join(format!("f{n}.png")))
                .expect("write png");
        }
    }
    played
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

// ---- fingerprints (same name-merged format as `dos_real.rs`) -----------------------------------------------

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

/// Compares (or, with `EMU_UPDATE_FINGERPRINTS`, merges) the fingerprint `name` of `tests/fingerprints/<file>`.
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
            "# {core}, generated media, no input; machine ROMs built into the core; core sha256 {}\n",
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
                "no fingerprint `{name}` in {}: inspect target/test-output/cpc-c64/*/*.png then run with EMU_UPDATE_FINGERPRINTS=1",
                path.display()
            )
        });
    assert_eq!(
        (want.1, want.2),
        (frame, fp),
        "fingerprint `{name}` differs; regenerate with EMU_UPDATE_FINGERPRINTS=1 if the change is legitimate"
    );
}

/// Plays the same session twice (two loads in this process), requires identical fingerprints, then pins the last
/// checkpoint under `name`. Returns the first run.
fn stable(
    file: &str,
    core: &str,
    name: &str,
    checkpoints: &[u64],
    mut session: impl FnMut(&Path) -> Played,
) -> Played {
    let a = session(&out_dir(&format!("{name}-a")));
    let b = session(&out_dir(&format!("{name}-b")));
    assert_eq!(a.prints, b.prints, "two runs of `{name}` must be identical");
    assert_eq!(a.ff, b.ff, "fast-forward requests of `{name}` are stable");
    let (frame, fp) = *a.prints.last().unwrap();
    assert_eq!(frame, *checkpoints.last().unwrap());
    check_fingerprint(file, core, name, frame, fp);
    a
}

const CPC: &str = "cap32";
const CPC_PRINTS: &str = "cpc-cap32.txt";
const C64: &str = "vice_x64";
const C64_PRINTS: &str = "c64-vice_x64.txt";
const C64SC: &str = "vice_x64sc";
const C64SC_PRINTS: &str = "c64-vice_x64sc.txt";

const BOOT: u64 = 300;

// ---- CPC ---------------------------------------------------------------------------------------------------

#[test]
#[ignore = "needs cap32: scripts/fetch-test-assets.sh --with-5.0"]
fn cpc_basic_without_content() {
    let _guard = lock();
    let played = stable(CPC_PRINTS, CPC, "basic", &[BOOT], |dir| {
        let mut emu = load(CPC, Machine::Cpc, ContentSpec::NoGame, &dir.join("data"));
        assert!(
            emu.system_info()
                .library_name
                .to_lowercase()
                .contains("cap32"),
            "{}",
            emu.system_info().library_name
        );
        let played = play(&mut emu, dir, &[BOOT]);
        assert!(not_blank(&emu));
        let av = emu.av_info();
        assert!((av.timing.fps - 50.0).abs() < 1.0, "fps {}", av.timing.fps);
        played
    });
    assert!(
        played.ff.is_empty(),
        "no fast-forward request: {:?}",
        played.ff
    );
}

/// Generated media of this test in a directory of its own.
fn generate(tag: &str) -> (PathBuf, PathBuf) {
    generated(&out_dir(&format!("gen-{tag}")))
}

#[test]
#[ignore = "needs cap32: scripts/fetch-test-assets.sh --with-5.0"]
fn cpc_disk_autorun_shows_the_program_text() {
    let _guard = lock();
    let (cpc, _) = generate("disk");
    let disk = cpc.join("hello.dsk");
    let before = sha(&disk);
    let played = stable(CPC_PRINTS, CPC, "hello-dsk", &[BOOT * 2], |dir| {
        let mut emu = load(
            CPC,
            Machine::Cpc,
            media(Machine::Cpc, "hello", &disk, None),
            &dir.join("data"),
        );
        let played = play(&mut emu, dir, &[BOOT * 2]);
        assert!(not_blank(&emu));
        played
    });
    assert!(played.ff.is_empty(), "{:?}", played.ff);
    assert_eq!(sha(&disk), before, "the source disk is never modified");
}

#[test]
#[ignore = "needs cap32: scripts/fetch-test-assets.sh --with-5.0"]
fn cpc_command_runs_the_second_program() {
    let _guard = lock();
    let (cpc, _) = generate("command");
    let two = cpc.join("two.dsk");
    // `launch` is written by the frontend as `#COMMAND:` of a generated playlist.
    stable(CPC_PRINTS, CPC, "second-launch", &[BOOT * 2], |dir| {
        let mut emu = load(
            CPC,
            Machine::Cpc,
            media(Machine::Cpc, "two", &two, Some("RUN\"SECOND\"")),
            &dir.join("data"),
        );
        let played = play(&mut emu, dir, &[BOOT * 2]);
        assert!(not_blank(&emu));
        played
    });
    // A user `.m3u` whose first line is a valid `#COMMAND:` keeps it.
    let m3u = cpc.join("command.m3u");
    stable(CPC_PRINTS, CPC, "second-m3u", &[BOOT * 2], |dir| {
        let mut emu = load(
            CPC,
            Machine::Cpc,
            playlist(Machine::Cpc, "command", &m3u),
            &dir.join("data"),
        );
        play(&mut emu, dir, &[BOOT * 2])
    });
}

#[test]
#[ignore = "needs cap32: scripts/fetch-test-assets.sh --with-5.0"]
fn cpc_cassette_load_asks_for_fast_forward_then_gives_it_back() {
    let _guard = lock();
    let (cpc, _) = generate("tape");
    let tape = cpc.join("hello.cdt");
    let before = sha(&tape);
    let checkpoints = [BOOT, 900, 2400];
    let played = stable(CPC_PRINTS, CPC, "hello-cdt", &checkpoints, |dir| {
        let mut emu = load(
            CPC,
            Machine::Cpc,
            media(Machine::Cpc, "hello", &tape, None),
            &dir.join("data"),
        );
        let played = play(&mut emu, dir, &checkpoints);
        assert!(not_blank(&emu));
        played
    });
    eprintln!("cdt prints {:?} ff {:?}", played.prints, played.ff);
    assert!(
        played.ff.first().is_some_and(|(_, on)| *on),
        "the core asked for fast-forward while loading: {:?}",
        played.ff
    );
    assert!(
        played.ff.last().is_some_and(|(_, on)| !*on),
        "and gave it back at the end of the load: {:?}",
        played.ff
    );
    assert_eq!(sha(&tape), before);
}

#[test]
#[ignore = "needs cap32: scripts/fetch-test-assets.sh --with-5.0"]
fn cpc_save_scope_game_keeps_writes_in_the_game_folder() {
    let _guard = lock();
    let (cpc, _) = generate("save");
    let disk = cpc.join("save.dsk");
    let before = sha(&disk);
    let dir = out_dir("save-run");
    let data = dir.join("data");
    let mut emu = load(
        CPC,
        Machine::Cpc,
        media(Machine::Cpc, "save", &disk, None),
        &data,
    );
    let folder = emu.identity().core_save_dir.clone();
    assert_eq!(
        folder,
        data.join("saves/cap32/save"),
        "<saves>/<core>/<game>"
    );
    assert!(folder.is_dir());
    play(&mut emu, &dir, &[BOOT * 2]);
    drop(emu);
    assert_eq!(sha(&disk), before, "the original image is intact");
    let names: Vec<String> = fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().any(|n| n.ends_with(".sav")),
        "differential `.sav` in the game folder: {names:?}"
    );
    let flat: Vec<_> = fs::read_dir(data.join("saves/cap32"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file() && !p.file_name().unwrap().to_string_lossy().starts_with('.'))
        .collect();
    assert!(flat.is_empty(), "nothing flat in the core folder: {flat:?}");
}

// ---- third-party homebrews: smoke tests ---------------------------------------------------------------------

/// Not blank at every checkpoint and the picture changes between the first and the last.
fn smoke(core: &str, machine: Machine, name: &str, spec: ContentSpec, checkpoints: &[u64]) {
    smoke_with(core, machine, name, spec, checkpoints, &mut NullInput);
}

fn smoke_with(
    core: &str,
    machine: Machine,
    name: &str,
    spec: ContentSpec,
    checkpoints: &[u64],
    input: &mut dyn InputBackend,
) {
    let dir = out_dir(&format!("smoke-{name}"));
    let mut emu = load(core, machine, spec, &dir.join("data"));
    let mut sink = CollectSink::default();
    let mut prints = Vec::new();
    for n in 1..=*checkpoints.last().unwrap() {
        emu.run_frame(input, &mut sink).expect("run_frame");
        if checkpoints.contains(&n) {
            assert!(not_blank(&emu), "{name}: blank screen at frame {n}");
            prints.push(emu.frame().fingerprint());
            emu.frame()
                .write_png(&dir.join(format!("f{n}.png")))
                .unwrap();
        }
    }
    assert_ne!(
        prints.first(),
        prints.last(),
        "{name}: the picture never changes: {prints:?}"
    );
}

fn rom(path: &str) -> PathBuf {
    require(&root().join("tests/roms").join(path)).to_path_buf()
}

/// Presses Return at frame `at` (the title screen asks for it) and moves the player right.
fn start_game(at: u64) -> ScriptedInput {
    ScriptedInput::new()
        .key(RetroKey::new(13).unwrap(), at..at + 10)
        .key(RetroKey::new(112).unwrap(), at + 100..at + 130)
}

#[test]
#[ignore = "needs cap32 and rpg-adventure: scripts/fetch-test-assets.sh --with-5.0"]
fn cpc_rpg_adventure_disk_snapshot_and_tape() {
    let _guard = lock();
    let (dsk, basic, cdt, sna) = (
        rom("cpc/rpg.dsk"),
        rom("cpc/rpg-basic.dsk"),
        rom("cpc/rpg.cdt"),
        rom("cpc/rpg.sna"),
    );
    // Each case: title screen (checkpoint 1), Return, the game (checkpoint 2).
    let cases: [(&str, ContentSpec, u64); 4] = [
        (
            "rpg-dsk",
            media(Machine::Cpc, "rpg", &dsk, Some("RUN\"RPG")),
            900,
        ),
        (
            "rpg-basic",
            media(Machine::Cpc, "rpg-basic", &basic, Some("RUN\"RPG")),
            900,
        ),
        ("rpg-cdt", media(Machine::Cpc, "rpg-tape", &cdt, None), 3000),
        (
            "rpg-sna",
            media(Machine::Cpc, "rpg-snapshot", &sna, None),
            200,
        ),
    ];
    for (name, spec, at) in cases {
        smoke_with(
            CPC,
            Machine::Cpc,
            name,
            spec,
            &[at - 50, at + 300],
            &mut start_game(at),
        );
    }
}

// ---- C64 ---------------------------------------------------------------------------------------------------

/// BASIC screen without content, then each generated medium on its own; the fingerprints are pinned per core.
fn c64_generated(core: &str, prints_file: &str, cases: &[(&str, &str, Option<&str>, u64)]) {
    let (_, c64) = generate(core);
    let basic = stable(prints_file, core, "basic", &[BOOT], |dir| {
        let mut emu = load(core, Machine::C64, ContentSpec::NoGame, &dir.join("data"));
        assert!(
            emu.system_info()
                .library_name
                .to_lowercase()
                .contains("vice"),
            "{}",
            emu.system_info().library_name
        );
        let played = play(&mut emu, dir, &[BOOT]);
        assert!(not_blank(&emu));
        played
    });
    let basic_print = basic.prints.last().unwrap().1;
    for &(name, file, launch, frames) in cases {
        let image = c64.join(file);
        let before = sha(&image);
        let played = stable(prints_file, core, name, &[frames], |dir| {
            let mut emu = load(
                core,
                Machine::C64,
                media(Machine::C64, name, &image, launch),
                &dir.join("data"),
            );
            let played = play(&mut emu, dir, &[frames]);
            assert!(not_blank(&emu));
            played
        });
        assert_ne!(
            played.prints.last().unwrap().1,
            basic_print,
            "{name}: still the BASIC screen"
        );
        assert_eq!(
            sha(&image),
            before,
            "{name}: the source file is never modified"
        );
    }
}

#[test]
#[ignore = "needs vice_x64: scripts/fetch-test-assets.sh --with-5.0"]
fn c64_basic_and_generated_media() {
    let _guard = lock();
    c64_generated(
        C64,
        C64_PRINTS,
        &[
            ("hello-prg", "hello.prg", None, 900),
            ("hello-d64", "hello.d64", None, 1500),
            ("hello-t64", "hello.t64", None, 1500),
            ("hello-tap", "hello.tap", None, 3000),
            ("cart-crt", "cart.crt", None, BOOT),
            // `<image>:<program>` first entry of the generated playlist.
            ("second-d64", "two.d64", Some("second"), 1500),
        ],
    );
}

#[test]
#[ignore = "needs vice_x64sc: scripts/fetch-test-assets.sh --with-5.0"]
fn c64sc_basic_and_hello_disk() {
    let _guard = lock();
    c64_generated(
        C64SC,
        C64SC_PRINTS,
        &[("hello-d64", "hello.d64", None, 1500)],
    );
}

#[test]
#[ignore = "needs vice_x64 and the C64 homebrews: scripts/fetch-test-assets.sh --with-5.0"]
fn c64_third_party_homebrews_run() {
    let _guard = lock();
    let (uberswap, kidelyneen, snake) = (
        rom("c64/uberswap.prg"),
        rom("c64/kidelyneen_AD.d64"),
        rom("c64/Wild.Snake.Boa.d64"),
    );
    // Autostart; the picture moves while the program loads, then at the title.
    for (name, file, frames) in [
        ("uberswap", uberswap, [600, 1500]),
        ("kidelyneen", kidelyneen, [600, 2400]),
        ("wild-snake", snake, [1200, 6000]),
    ] {
        smoke(
            C64,
            Machine::C64,
            name,
            media(Machine::C64, name, &file, None),
            &frames,
        );
    }
}

#[test]
#[ignore = "needs vice_x64: scripts/fetch-test-assets.sh --with-5.0"]
fn c64_state_size_depends_on_the_media_and_states_cross_load() {
    let _guard = lock();
    let (_, c64) = generate("state");
    let disk = c64.join("hello.d64");
    let dir = out_dir("state-run");
    let session = |spec: ContentSpec, tag: &str| {
        let mut emu = load(C64, Machine::C64, spec, &dir.join(tag));
        run_frames(&mut emu, BOOT);
        emu
    };
    let mut bare = session(ContentSpec::NoGame, "bare");
    let without_disk = bare.serialize_state().unwrap();
    drop(bare);
    let mut with = session(media(Machine::C64, "hello", &disk, None), "disk");
    let with_disk = with.serialize_state().unwrap();
    assert_ne!(
        without_disk.len(),
        with_disk.len(),
        "VICE states vary with the inserted media (variable_state_size)"
    );
    with.unserialize_state(&without_disk)
        .expect("state saved without a disk, loaded with one");
    run_frames(&mut with, 30);
    drop(with);
    let mut bare = session(ContentSpec::NoGame, "bare-again");
    bare.unserialize_state(&with_disk)
        .expect("state saved with a disk, loaded without");
    run_frames(&mut bare, 30);
    assert!(not_blank(&bare));
}
