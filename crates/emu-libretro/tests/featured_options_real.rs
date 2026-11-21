//! Option audit (PQ2b): for every real core present in `tests/cores/` and described in
//! `crates/emu-assets/data/cores.toml`, every key and value the frontend relies on must exist in the options the
//! core declares (as seen by emu-libretro, legacy / v1 / v2): `featured_options`, `options`,
//! `options_by_extension`, `layout_option` (key, fallback, map values) and `required_firmware.fallback`.
//! Options are enumerated twice, before and after `load_game`, to tell when a core declares them.
//!
//! Each core runs in a child process (this binary re-executed, `zz_worker`), as in `reload_real`.
//! `#[ignore]`: run `scripts/fetch-test-assets.sh --with-all` then
//! `cargo test -p emu-libretro --test featured_options_real -- --ignored --test-threads=1 --nocapture`.
//! A core without library file is skipped. An option whose value list is empty (free text) accepts any value.

#![cfg(target_os = "linux")]
// Linking libm needs an `extern` block (test binary only; emu-libretro is allowed `unsafe`).
#![allow(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use emu_libretro::{Core, CoreConfig, FrontendHandler, InputQuery, VideoFrame};

// puae cores expect the host to provide libm; a referenced symbol keeps `--as-needed` from dropping it.
#[link(name = "m")]
unsafe extern "C" {
    safe fn cosf(x: f32) -> f32;
}

/// Test data for each core, relative to `tests/roms` (`None`: enumerate without a game only).
fn rom_for(core: &str) -> Option<&'static str> {
    Some(match core {
        "genesis_plus_gx" | "picodrive" => "240pSuite-1.21.bin",
        "snes9x" => "240pSuite-SNES-1.02.sfc",
        "gambatte" => "gb240p-0.23.gb",
        "mesen" | "fceumm" | "nestopia" => "240pee-0.23.nes",
        "mgba" => "gba/hello.gba",
        "sameboy" => "cgb-acid2-1.1.gbc",
        "pcsx_rearmed" | "swanstation" | "mednafen_psx" => "psx/240pTestSuitePS1-EMU.cue",
        "hatari" => "nova-defense.st",
        "puae" | "puae2021" => "amiga/boot.adf",
        "dosbox_pure" => "dos/matrix62.zip",
        "cap32" => "cpc/rpg.dsk",
        "vice_x64" | "vice_x64sc" => "c64/uberswap.prg",
        _ => return None,
    })
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Quiet;

impl FrontendHandler for Quiet {
    fn video_refresh(&mut self, _frame: Option<VideoFrame<'_>>) {}
    fn audio_samples(&mut self, _samples: &[i16]) {}
    fn input_poll(&mut self) {}
    fn input_state(&mut self, _query: InputQuery) -> i16 {
        0
    }
}

fn dump(c: &mut Core, phase: &str) {
    for o in c.options() {
        let values: Vec<String> = o.values.iter().map(|v| v.value.clone()).collect();
        println!(
            "@@ {phase}\t{}\t{}\t{}",
            o.key,
            o.default,
            values.join("\u{1f}")
        );
    }
}

#[test]
#[ignore = "worker of `featured_options_exist_in_every_real_core`; needs OPTIONS_CASE"]
fn zz_worker() {
    let Some(name) = std::env::var_os("OPTIONS_CASE") else {
        return;
    };
    std::hint::black_box(cosf(std::hint::black_box(0.5)));
    let name = name.to_string_lossy().into_owned();
    let dir = root().join("target/test-output/options").join(&name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("system")).unwrap();
    std::fs::create_dir_all(dir.join("saves")).unwrap();
    if name == "hatari" {
        std::fs::copy(
            root().join("tests/roms/system/tos.img"),
            dir.join("system/tos.img"),
        )
        .unwrap();
    }
    if name == "opera" {
        // opera_bios lists the BIOS files found in system/ when the core starts: a placeholder makes the
        // value `panafz10.bin` selectable (the core only scans names here).
        std::fs::write(dir.join("system/panafz10.bin"), b"placeholder").unwrap();
    }
    let config = CoreConfig {
        system_dir: dir.join("system"),
        save_dir: dir.join("saves"),
        assets_dir: dir.join("assets"),
        variables: Vec::new(),
        provide_vfs: false,
        copy_dir: Some(dir.join("copies")),
        fastforward_override: false,
        variable_state_size: false,
    };
    let lib = root().join(format!("tests/cores/{name}_libretro.so"));
    eprintln!("worker: loading {name}");
    let mut c = Core::load(&lib, config).unwrap_or_else(|e| panic!("load: {e}"));
    eprintln!("worker: loaded");
    dump(&mut c, "pre");
    let rom = rom_for(&name).map(|r| root().join("tests/roms").join(r));
    match rom {
        Some(rom) if rom.is_file() => match c.load_game(&rom) {
            Ok(_) => {
                println!("@@ game\tloaded");
                dump(&mut c, "post-load");
                let mut h = Quiet;
                for _ in 0..5 {
                    let _ = c.run(&mut h);
                }
                dump(&mut c, "post-run");
            }
            Err(e) => println!("@@ game\tload_game failed: {e}"),
        },
        _ => println!("@@ game\tno test data"),
    }
}

// ---- driver side ----

type Table = toml::map::Map<String, toml::Value>;

/// (what, key, optional wanted value)
type Expect = (String, String, Option<String>);

fn str_list(v: &toml::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn expectations(core: &Table) -> Vec<Expect> {
    let mut out = Vec::new();
    if let Some(t) = core.get("featured_options").and_then(|v| v.as_table()) {
        for (system, keys) in t {
            for k in str_list(keys) {
                out.push((format!("featured_options.{system}"), k, None));
            }
        }
    }
    if let Some(t) = core.get("options").and_then(|v| v.as_table()) {
        for (k, v) in t {
            out.push(("options".into(), k.clone(), v.as_str().map(str::to_owned)));
        }
    }
    if let Some(t) = core.get("options_by_extension").and_then(|v| v.as_table()) {
        for (ext, opts) in t {
            for (k, v) in opts.as_table().into_iter().flatten() {
                out.push((
                    format!("options_by_extension.{ext}"),
                    k.clone(),
                    v.as_str().map(str::to_owned),
                ));
            }
        }
    }
    if let Some(l) = core.get("layout_option").and_then(|v| v.as_table())
        && let Some(key) = l.get("option").and_then(|v| v.as_str())
    {
        let mut values: Vec<String> = l
            .get("map")
            .and_then(|m| m.as_table())
            .into_iter()
            .flatten()
            .filter_map(|(_, v)| v.as_str().map(str::to_owned))
            .collect();
        values.extend(
            l.get("fallback")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
        );
        for v in values {
            out.push(("layout_option".into(), key.to_owned(), Some(v)));
        }
    }
    if let Some(t) = core.get("required_firmware").and_then(|v| v.as_table()) {
        for (system, fw) in t {
            let fb = fw.get("fallback").and_then(|v| v.as_table());
            if let Some(fb) = fb
                && let Some(k) = fb.get("option").and_then(|v| v.as_str())
            {
                out.push((
                    format!("required_firmware.{system}.fallback"),
                    k.to_owned(),
                    fb.get("value").and_then(|v| v.as_str()).map(str::to_owned),
                ));
            }
        }
    }
    out
}

/// key -> (default, allowed values) for one phase.
type Phase = BTreeMap<String, (String, Vec<String>)>;

fn parse(stdout: &str) -> (BTreeMap<String, Phase>, String) {
    let mut phases: BTreeMap<String, Phase> = BTreeMap::new();
    let mut game = String::new();
    // The harness may glue its own "test ... " prefix before the first line, and cores print on stdout.
    for l in stdout
        .lines()
        .filter_map(|l| l.find("@@ ").map(|i| &l[i + 3..]))
    {
        let f: Vec<&str> = l.splitn(4, '\t').collect();
        if f[0] == "game" {
            game = f[1].to_owned();
            continue;
        }
        if f.len() < 3 {
            eprintln!("  note: {l}");
            continue;
        }
        let values = f
            .get(3)
            .filter(|v| !v.is_empty())
            .map(|v| v.split('\u{1f}').map(str::to_owned).collect())
            .unwrap_or_default();
        phases
            .entry(f[0].to_owned())
            .or_default()
            .insert(f[1].to_owned(), (f[2].to_owned(), values));
    }
    (phases, game)
}

#[test]
#[ignore = "needs tests/cores (scripts/fetch-test-assets.sh --with-all)"]
fn featured_options_exist_in_every_real_core() {
    let text = std::fs::read_to_string(root().join("crates/emu-assets/data/cores.toml")).unwrap();
    let doc: Table = toml::from_str(&text).unwrap();
    let cores = doc["cores"].as_table().expect("[cores]");
    let mut failures = Vec::new();
    let mut ran = 0;
    for (name, core) in cores {
        let Some(core) = core.as_table() else {
            continue;
        };
        let lib = root().join(format!("tests/cores/{name}_libretro.so"));
        if !lib.is_file() {
            eprintln!("SKIP {name}: {} missing", lib.display());
            continue;
        }
        let run = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "zz_worker",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("OPTIONS_CASE", name)
            .output()
            .expect("spawn the worker");
        if !run.status.success() {
            failures.push(format!(
                "{name}: worker {}\n{}",
                run.status,
                String::from_utf8_lossy(&run.stderr)
            ));
            continue;
        }
        ran += 1;
        let (phases, game) = parse(&String::from_utf8_lossy(&run.stdout));
        let pre = phases.get("pre").cloned().unwrap_or_default();
        // Final view: the last phase that exists (options may be declared or amended by the game).
        let last = ["post-run", "post-load", "pre"]
            .iter()
            .find_map(|p| phases.get(*p))
            .cloned()
            .unwrap_or_default();
        eprintln!(
            "{name}: game={game}; options pre={} post-load={} post-run={}",
            pre.len(),
            phases.get("post-load").map_or(0, BTreeMap::len),
            phases.get("post-run").map_or(0, BTreeMap::len)
        );
        for (what, key, value) in expectations(core) {
            let when = if pre.contains_key(&key) {
                "declared at load"
            } else {
                "NOT declared before load_game"
            };
            match last.get(&key) {
                None => failures.push(format!(
                    "{name}: {what}: option `{key}` does not exist (game: {game})"
                )),
                Some((_, allowed)) => {
                    if !pre.contains_key(&key) && !pre.is_empty() {
                        eprintln!(
                            "  NOTE {name}: `{key}` {when} (declared after the game was loaded)"
                        );
                    }
                    if let Some(v) = value
                        && !allowed.is_empty()
                        && !allowed.contains(&v)
                    {
                        failures.push(format!(
                            "{name}: {what}: value `{v}` of `{key}` refused, allowed: {allowed:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(ran > 0, "no core present");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
