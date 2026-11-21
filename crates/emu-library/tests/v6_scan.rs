//! 6.0 scan (design v6.0 §3.4, §5): GBA, 32X (`.32x`, `.bin` by header, zipped), Mega CD Redump identification
//! (track size + CRC32, header serial, name), region (header, Redump, name), `.chd` by name, multi-CD.
//! Discs and the 32X header come from `tests/tools/mkcd.py` (original data, no commercial content, no network).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;

use emu_library::{
    BinKind, Dat, DatEntry, DatSource, DiscFormat, DiscRegion, Game, Identification, Library,
    ScanOptions, ScanReport, disc_serial, probe_bin,
};

fn mkcd(args: &[&str]) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/tools/mkcd.py");
    let out = Command::new("python3")
        .arg("-I")
        .arg(script)
        .args(args)
        .output()
        .expect("python3 is required for the Mega CD fixtures");
    assert!(
        out.status.success(),
        "mkcd.py {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn scan(lib: &mut Library, root: &Path) -> ScanReport {
    let opts = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn listed(lib: &Library, root: &Path) -> BTreeSet<(String, String)> {
    lib.games()
        .unwrap()
        .into_iter()
        .map(|g| {
            let rel = g.path.strip_prefix(root).unwrap().to_string_lossy();
            (g.system, rel.replace('\\', "/"))
        })
        .collect()
}

fn pair(system: &str, path: &str) -> (String, String) {
    (system.to_owned(), path.to_owned())
}

fn game(lib: &Library, file: &str) -> Game {
    lib.games()
        .unwrap()
        .into_iter()
        .find(|g| g.path.file_name().is_some_and(|n| n == file))
        .unwrap_or_else(|| panic!("{file} not listed"))
}

fn cartridge() -> Vec<u8> {
    let mut rom = vec![0u8; 0x4000];
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom
}

/// Stored (uncompressed) zip of one entry.
fn zip_of(name: &str, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut common = Vec::new();
    for v in [20u16, 0, 0, 0, 0] {
        common.extend_from_slice(&v.to_le_bytes());
    }
    common.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
    common.extend_from_slice(&(data.len() as u32).to_le_bytes());
    common.extend_from_slice(&(data.len() as u32).to_le_bytes());
    common.extend_from_slice(&(name.len() as u16).to_le_bytes());
    common.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    out.extend_from_slice(&common);
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(data);
    let cd = out.len() as u32;
    let mut central = Vec::new();
    central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    central.extend_from_slice(&20u16.to_le_bytes());
    central.extend_from_slice(&common);
    central.extend_from_slice(&[0u8; 10]);
    central.extend_from_slice(&0u32.to_le_bytes());
    central.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn redump(
    game: &str,
    rom: &str,
    data: Option<&[u8]>,
    serial: Option<&str>,
    region: &str,
) -> DatEntry {
    DatEntry {
        game: game.to_owned(),
        rom_name: rom.to_owned(),
        size: data.map_or(1, |d| d.len() as u64),
        crc32: data.map_or(0, crc32fast::hash),
        sha1: None,
        serial: serial.map(str::to_owned),
        region: Some(region.to_owned()),
    }
}

#[test]
fn gba_and_32x_cartridges_and_bins_by_header() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    fs::create_dir_all(root.join("megadrive")).unwrap();
    fs::create_dir_all(root.join("32x")).unwrap();
    fs::write(root.join("Hello.gba"), [0x2Eu8; 512]).unwrap();
    mkcd(&["32x", &r("Fake.32x")]);
    mkcd(&["32x", &r("Loose 32X.bin")]);
    mkcd(&["32x", &r("megadrive/In MD Folder.bin")]);
    mkcd(&["32x", &r("32x/Pinned.bin")]);
    let x = fs::read(root.join("Loose 32X.bin")).unwrap();
    assert_eq!(
        probe_bin(&root.join("Loose 32X.bin")).unwrap(),
        BinKind::Sega32x
    );
    fs::write(root.join("Zipped 32X.zip"), zip_of("game.bin", &x)).unwrap();
    fs::write(root.join("Sonic.bin"), cartridge()).unwrap();
    fs::write(root.join("32x/Cart In 32X Folder.bin"), cartridge()).unwrap();

    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    let want = BTreeSet::from([
        pair("gba", "Hello.gba"),
        pair("32x", "Fake.32x"),
        pair("32x", "Loose 32X.bin"),
        pair("32x", "megadrive/In MD Folder.bin"),
        pair("32x", "32x/Pinned.bin"),
        pair("32x", "Zipped 32X.zip"),
        pair("megadrive", "Sonic.bin"),
        // The folder named `32x` does not make a Mega Drive cartridge a 32X game (1.0 rule).
        pair("megadrive", "32x/Cart In 32X Folder.bin"),
    ]);
    assert_eq!(listed(&lib, &root), want);
    assert!(lib.games().unwrap().iter().all(|g| g.region.is_none()));
    // Nothing changes on a second scan, and nothing is analysed again.
    let again = scan(&mut lib, &root);
    assert_eq!(
        (again.unchanged, again.added, again.updated),
        (8, 0, 0),
        "{again:?}"
    );
    assert_eq!(listed(&lib, &root), want);

    // A 32X DAT import re-identifies the 32X `.bin` (by its hashes) without moving it to the Mega Drive.
    let dat = Dat {
        name: "Sega - 32X".into(),
        version: None,
        entries: vec![redump(
            "Fake 32X (World)",
            "Fake 32X (World).32x",
            Some(&x),
            None,
            "World",
        )],
    };
    lib.import_dat("32x", &dat, None).unwrap();
    let loose = game(&lib, "Loose 32X.bin");
    assert_eq!(
        (
            loose.system.as_str(),
            loose.title.as_str(),
            loose.identification
        ),
        ("32x", "Fake 32X (World)", Identification::Crc32Size)
    );
    lib.import_dat("megadrive", &Dat::default(), None).unwrap();
    assert_eq!(game(&lib, "Loose 32X.bin").system, "32x");
    assert_eq!(game(&lib, "Sonic.bin").system, "megadrive");
}

#[test]
fn a_32x_bin_stored_as_mega_drive_by_an_older_scan_moves_to_32x() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    fs::create_dir_all(&root).unwrap();
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    mkcd(&["32x", &r("Old.bin")]);
    let db = tmp.path().join("library.sqlite");
    let (mut lib, _) = Library::open(&db).unwrap();
    scan(&mut lib, &root);
    drop(lib);
    // As a 5.0 scan stored it.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute("UPDATE game SET system = 'megadrive'", [])
        .unwrap();
    let (mut lib, _) = Library::open(&db).unwrap();
    assert_eq!(game(&lib, "Old.bin").system, "megadrive");
    let report = scan(&mut lib, &root);
    assert_eq!(report.updated, 1, "{report:?}");
    assert_eq!(game(&lib, "Old.bin").system, "32x");
}

/// Track 01, header serial and name, each against an extract of a Redump DAT; region by header, DAT, then name.
#[test]
fn redump_identification_and_region() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    mkcd(&[
        "megacd",
        &r("megacd/crc"),
        "--name",
        "Crc Disc",
        "--region",
        "J",
    ]);
    mkcd(&[
        "megacd",
        &r("megacd/serial"),
        "--name",
        "Serial Disc",
        "--region",
        "U",
        "--serial",
        "T-93175 -00",
    ]);
    mkcd(&[
        "megacd",
        &r("megacd/plain"),
        "--name",
        "Plain (Europe)",
        "--region",
        "E",
    ]);
    fs::write(root.join("Chd Game (Europe).chd"), b"not read").unwrap();
    fs::write(root.join("Unknown (USA).chd"), b"not read").unwrap();
    fs::create_dir_all(root.join("megacd")).unwrap();
    fs::write(root.join("megacd/Bare.chd"), b"not read").unwrap();
    fs::write(root.join("megacd/Tagged (Japan).chd"), b"not read").unwrap();
    assert_eq!(
        disc_serial(
            &root.join("megacd/serial/Serial Disc.cue"),
            DiscFormat::SegaCd
        )
        .unwrap()
        .as_deref(),
        Some("T-93175")
    );

    let mut lib = Library::open_in_memory().unwrap();
    // Without DAT: the `.chd` outside `megacd/` are unclassified; regions come from the headers and names.
    let report = scan(&mut lib, &root);
    assert_eq!(report.unclassified, 2, "{report:?}");
    let region = |lib: &Library, file: &str| game(lib, file).region;
    assert_eq!(region(&lib, "Crc Disc.cue"), Some(DiscRegion::Japan));
    assert_eq!(region(&lib, "Serial Disc.cue"), Some(DiscRegion::Usa));
    assert_eq!(region(&lib, "Plain (Europe).cue"), Some(DiscRegion::Europe));
    assert_eq!(region(&lib, "Bare.chd"), None);
    assert_eq!(region(&lib, "Tagged (Japan).chd"), Some(DiscRegion::Japan));

    let track = fs::read(root.join("megacd/crc/Crc Disc (Track 1).bin")).unwrap();
    let dat = Dat {
        name: "Sega - Mega-CD - Sega CD".into(),
        version: Some("20261121".into()),
        entries: vec![
            redump(
                "Crc Disc (Japan)",
                "Crc Disc (Japan) (Track 01).bin",
                Some(&track),
                None,
                "Japan",
            ),
            redump(
                "Serial Disc (Europe)",
                "Serial Disc (Europe) (Track 01).bin",
                None,
                Some("T-93175"),
                "Europe",
            ),
            redump(
                "Chd Game (Europe)",
                "Chd Game (Europe) (Track 01).bin",
                None,
                None,
                "Japan",
            ),
            redump("Bare", "Bare (Track 01).bin", None, None, "USA"),
        ],
    };
    lib.import_dat_source("megacd", DatSource::Redump, &dat, None)
        .unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(report.unclassified, 1, "{report:?}");
    let id = |lib: &Library, file: &str| {
        let g = game(lib, file);
        (g.system, g.title, g.identification, g.region)
    };
    assert_eq!(
        id(&lib, "Crc Disc.cue"),
        (
            "megacd".into(),
            "Crc Disc (Japan)".into(),
            Identification::Crc32Size,
            Some(DiscRegion::Japan)
        )
    );
    // The header region (USA byte) wins over the DAT line (Europe).
    assert_eq!(
        id(&lib, "Serial Disc.cue"),
        (
            "megacd".into(),
            "Serial Disc (Europe)".into(),
            Identification::Serial,
            Some(DiscRegion::Usa)
        )
    );
    // A `.chd` is never read: attributed by its name, region of the DAT line before the name tag.
    assert_eq!(
        id(&lib, "Chd Game (Europe).chd"),
        (
            "megacd".into(),
            "Chd Game (Europe)".into(),
            Identification::Name,
            Some(DiscRegion::Japan)
        )
    );
    assert_eq!(
        id(&lib, "Bare.chd"),
        (
            "megacd".into(),
            "Bare".into(),
            Identification::Name,
            Some(DiscRegion::Usa)
        )
    );
    assert_eq!(
        id(&lib, "Plain (Europe).cue"),
        (
            "megacd".into(),
            "Plain (Europe)".into(),
            Identification::Unknown,
            Some(DiscRegion::Europe)
        )
    );
    // Cached: nothing is analysed again.
    let again = scan(&mut lib, &root);
    assert_eq!((again.added, again.updated), (0, 0), "{again:?}");
    assert_eq!(id(&lib, "Crc Disc.cue").2, Identification::Crc32Size);
}

#[test]
fn multi_cd_playlists_and_disc_groups() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    // A playlist outside `megacd/`: its system is the one of its entries (Sega CD header).
    mkcd(&[
        "megacd",
        &r("elsewhere"),
        "--name",
        "Two Discs",
        "--region",
        "E",
        "--discs",
        "2",
    ]);
    // `(Disc N)` files without playlist, in `megacd/`: one game.
    mkcd(&[
        "megacd",
        &r("megacd/group"),
        "--name",
        "Grouped (Japan)",
        "--region",
        "J",
        "--discs",
        "2",
    ]);
    fs::remove_file(root.join("megacd/group/Grouped (Japan).m3u")).unwrap();

    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(
        listed(&lib, &root),
        BTreeSet::from([
            pair("megacd", "elsewhere/Two Discs.m3u"),
            pair("megacd", "megacd/group/Grouped (Japan) (Disc 1).cue"),
        ])
    );
    let m3u = game(&lib, "Two Discs.m3u");
    assert_eq!((m3u.media.len(), m3u.region), (2, Some(DiscRegion::Europe)));
    let group = game(&lib, "Grouped (Japan) (Disc 1).cue");
    assert_eq!(group.media.len(), 2);
    assert_eq!(
        (group.title.as_str(), group.region),
        ("Grouped (Japan)", Some(DiscRegion::Japan))
    );
    let again = scan(&mut lib, &root);
    assert_eq!(
        (again.unchanged, again.added, again.updated),
        (2, 0, 0),
        "{again:?}"
    );
}

/// DOS keeps its CD rules: a `.cue` under `dos/` is DOS, a `.chd` outside any folder named like a Redump game of
/// the Mega CD is not taken by DOS.
#[test]
fn dos_discs_are_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let r = |s: &str| root.join(s).to_string_lossy().into_owned();
    mkcd(&[
        "megacd",
        &r("dos/Game"),
        "--name",
        "Game (Disc 1)",
        "--region",
        "E",
    ]);
    mkcd(&[
        "megacd",
        &r("dos/Game"),
        "--name",
        "Game (Disc 2)",
        "--region",
        "E",
    ]);
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    let games = lib.games().unwrap();
    assert!(
        games
            .iter()
            .all(|g| g.system == "dos" && g.region.is_none() && g.media.is_empty()),
        "{games:?}"
    );
}
