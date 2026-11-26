//! Unit tests of the tidy plan, `create_system_folders`, `last_tidy`, `Library::relocate` and the moved-file net
//! of the scan. Every tree is generated in a temporary folder (no commercial data).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::*;
use crate::disc::fixtures::{opera_sector0, psx_iso, to_raw};
use crate::{CoverKind, CoverState, Library, ScanOptions};

const CNF: &[u8] = b"BOOT = cdrom:\\SLES_000.32;1\r\n";
const EUROPE: &str = "Sony Computer Entertainment Euro pe";

pub(super) fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

pub(super) fn psx_track() -> Vec<u8> {
    to_raw(&psx_iso(Some(EUROPE), Some(CNF)), 2)
}

fn opera_track() -> Vec<u8> {
    let mut iso = opera_sector0();
    iso.resize(8 * 2048, 0x11);
    to_raw(&iso, 1)
}

pub(super) fn cue(tracks: &[&str]) -> Vec<u8> {
    let mut text = String::new();
    for (i, t) in tracks.iter().enumerate() {
        let kind = if i == 0 { "MODE2/2352" } else { "AUDIO" };
        text.push_str(&format!(
            "FILE \"{t}\" BINARY\n  TRACK {:02} {kind}\n",
            i + 1
        ));
    }
    text.into_bytes()
}

/// Writes `<dir>/<name>.cue` and its tracks (first one a data track built by `data`).
pub(super) fn cd(dir: &Path, name: &str, audio: usize, data: fn() -> Vec<u8>) {
    let mut tracks = vec![format!("{name} (Track 1).bin")];
    tracks.extend((0..audio).map(|i| format!("{name} (Track {}).bin", i + 2)));
    let refs: Vec<&str> = tracks.iter().map(String::as_str).collect();
    write(&dir.join(format!("{name}.cue")), &cue(&refs));
    write(&dir.join(&tracks[0]), &data());
    for t in &tracks[1..] {
        write(&dir.join(t), &[0u8; 4096]);
    }
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        w.start_file(*name, options).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// Pseudo-random bytes (no header of any kind).
pub(super) fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        })
        .collect()
}

struct Fixture {
    _tmp: tempfile::TempDir,
    roms: PathBuf,
    journals: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    fs::create_dir(&roms).unwrap();
    let journals = tmp.path().join("rangement");
    Fixture {
        _tmp: tmp,
        roms,
        journals,
    }
}

impl Fixture {
    fn options(&self) -> TidyOptions {
        TidyOptions::new(&self.roms, &self.journals).unwrap()
    }

    fn plan(&self, library: &Library) -> TidyPlan {
        library
            .plan_tidy(&self.options(), &AtomicBool::new(false), &mut |_| {})
            .unwrap()
    }
}

fn unit<'p>(plan: &'p TidyPlan, primary: &str) -> &'p TidyUnit {
    let wanted = PathBuf::from(primary);
    plan.units
        .iter()
        .zip(&plan.meta)
        .find(|(u, m)| {
            m.files.first().is_some_and(|f| f.path == wanted)
                || u.whole_dir.as_ref().is_some_and(|w| w.from == wanted)
                || (m.files.is_empty() && u.title == primary)
        })
        .map(|(u, _)| u)
        .unwrap_or_else(|| panic!("no unit for {primary}: {:#?}", plan.units))
}

fn rel(s: &str) -> PathBuf {
    s.split('/').collect()
}

fn tos(unit: &TidyUnit) -> Vec<PathBuf> {
    unit.moves.iter().map(|m| m.to.clone()).collect()
}

fn insert_game(library: &Library, key: &Path, system: &str, title: &str) -> GameId {
    library
        .conn
        .execute(
            "INSERT INTO game (path, system, size, file_size, mtime_unix, title, identification)
             VALUES (?1, ?2, 1, 1, 0, ?3, 'unknown')",
            rusqlite::params![key.to_str().unwrap(), system, title],
        )
        .unwrap();
    GameId(library.conn.last_insert_rowid())
}

/// `(relative path → (kind, size, mtime, link target))` of everything below `dir`, links not followed.
fn snapshot(
    dir: &Path,
) -> BTreeMap<PathBuf, (char, u64, Option<std::time::SystemTime>, Option<PathBuf>)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let meta = fs::symlink_metadata(e.path()).unwrap();
            let kind = if meta.file_type().is_symlink() {
                'l'
            } else if meta.is_dir() {
                stack.push(e.path());
                'd'
            } else {
                'f'
            };
            out.insert(
                e.path().strip_prefix(dir).unwrap().to_path_buf(),
                (
                    kind,
                    meta.len(),
                    meta.modified().ok(),
                    fs::read_link(e.path()).ok(),
                ),
            );
        }
    }
    out
}

/// The tree of the « vrac » case (ADR 0023 §4.2): every kind of unit at once.
fn messy_tree(roms: &Path) {
    write(&roms.join("Super Game (Europe).sfc"), &noise(4096, 1));
    write(&roms.join("Pocket.gb"), &noise(2048, 2));
    write(&roms.join("Known.gb"), &noise(2048, 3));
    write(
        &roms.join("Mario.zip"),
        &zip_of(&[("Mario.nes", &noise(1024, 4)[..])]),
    );
    write(
        &roms.join("Doom.zip"),
        &zip_of(&[
            ("DOOM.EXE", &noise(512, 5)[..]),
            ("DOOM.WAD", &noise(512, 6)[..]),
        ]),
    );
    write(
        &roms.join("Disc.zip"),
        &zip_of(&[("game.iso", &psx_iso(None, Some(CNF))[..])]),
    );
    cd(&roms.join("FF (Europe)"), "FF (Europe)", 1, psx_track);
    cd(roms, "Saga (Disc 1)", 0, psx_track);
    cd(roms, "Saga (Disc 2)", 0, psx_track);
    cd(roms, "Road (Disc 1)", 0, opera_track);
    cd(roms, "Road (Disc 2)", 0, opera_track);
    write(&roms.join("Dungeon (Disk 1 of 2).st"), &noise(1000, 7));
    write(&roms.join("Dungeon (Disk 2 of 2).st"), &noise(1000, 8));
    write(&roms.join("Worms.adf"), &noise(1000, 9));
    write(&roms.join("Jeux/Commander Keen/KEEN.EXE"), &noise(700, 10));
    write(
        &roms.join("Jeux/Commander Keen/data/LEVEL1.DAT"),
        &noise(700, 11),
    );
    write(&roms.join("scph5501.bin"), &noise(512 * 1024, 12));
    write(&roms.join("kick13.rom"), &noise(1024, 13));
    write(&roms.join("notes.txt"), b"hello");
    write(&roms.join("Vagrant.chd"), &noise(3000, 14));
    write(&roms.join("Vagrant.sbi"), &noise(30, 15));
    write(&roms.join("snes/Old.sfc"), &noise(100, 16));
    fs::create_dir(roms.join("gb")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(roms.join("Super Game (Europe).sfc"), roms.join("link.sfc"))
        .unwrap();
}

#[test]
fn messy_root_plan_is_complete_and_writes_nothing() {
    let f = fixture();
    messy_tree(&f.roms);
    let db = f._tmp.path().join("library.sqlite");
    let (library, _) = Library::open(&db).unwrap();
    let known = insert_game(&library, &f.roms.join("Known.gb"), "gbc", "Known Game");
    let games_before = library.games().unwrap();
    let tree_before = snapshot(f._tmp.path());

    let mut calls = 0;
    let plan = library
        .plan_tidy(&f.options(), &AtomicBool::new(false), &mut |_| calls += 1)
        .unwrap();

    assert_eq!(
        snapshot(f._tmp.path()),
        tree_before,
        "the plan wrote on disk"
    );
    assert_eq!(
        library.games().unwrap(),
        games_before,
        "the plan wrote in the base"
    );
    assert!(!f.journals.exists());
    assert!(calls >= 1);

    let sure = |r| TidyStatus::Sure(r);
    let snes = unit(&plan, "Super Game (Europe).sfc");
    assert_eq!(snes.status, sure(TidyReason::Extension));
    assert_eq!(tos(snes), [rel("snes/Super Game (Europe).sfc")]);

    let pocket = unit(&plan, "Pocket.gb");
    assert_eq!(pocket.status, TidyStatus::Ambiguous);
    assert_eq!(pocket.candidates, ["gb", "gbc"]);
    assert!(pocket.moves.is_empty() && pocket.system.is_none());

    let known_unit = unit(&plan, "Known.gb");
    assert_eq!(known_unit.status, sure(TidyReason::Library));
    assert_eq!(known_unit.known_game, Some(known));
    assert_eq!(known_unit.system.as_deref(), Some("gbc"));
    assert_eq!(known_unit.candidates, ["gb"]);
    assert_eq!(known_unit.title, "Known Game");
    assert_eq!(tos(known_unit), [rel("gbc/Known.gb")]);

    let mario = unit(&plan, "Mario.zip");
    assert_eq!(mario.status, sure(TidyReason::ArchiveContent));
    assert_eq!(tos(mario), [rel("nes/Mario.zip")]);

    let doom = unit(&plan, "Doom.zip");
    assert_eq!(
        doom.status,
        TidyStatus::Suggested(TidyReason::ArchiveContent)
    );
    assert_eq!(doom.system.as_deref(), Some("dos"));

    assert_eq!(
        unit(&plan, "Disc.zip").status,
        TidyStatus::Blocked(TidyBlock::DiscInArchive)
    );

    // A CD alone in its folder: moved whole, under the folder's name.
    let ff = unit(&plan, "FF (Europe)");
    assert_eq!(ff.status, sure(TidyReason::Header));
    assert_eq!(
        ff.whole_dir.as_ref().map(|w| (&w.from, &w.to)),
        Some((&rel("FF (Europe)"), &rel("psx/FF (Europe)")))
    );
    assert_eq!(ff.moves.len(), 3);
    assert!(tos(ff).contains(&rel("psx/FF (Europe)/FF (Europe) (Track 2).bin")));

    // Two CDs without `.m3u`: one unit, one folder (the scan then sees one game).
    let saga = unit(&plan, "Saga (Disc 1).cue");
    assert_eq!(saga.system.as_deref(), Some("psx"));
    assert_eq!(saga.moves.len(), 4);
    assert!(tos(saga).iter().all(|t| t.starts_with(rel("psx/Saga"))));
    assert_eq!(saga.title, "Saga");
    // 3DO does not group its discs: one unit per disc.
    let road1 = unit(&plan, "Road (Disc 1).cue");
    let road2 = unit(&plan, "Road (Disc 2).cue");
    assert_eq!(road1.system.as_deref(), Some("3do"));
    assert!(
        tos(road1)
            .iter()
            .all(|t| t.starts_with(rel("3do/Road (Disc 1)")))
    );
    assert!(
        tos(road2)
            .iter()
            .all(|t| t.starts_with(rel("3do/Road (Disc 2)")))
    );

    let st = unit(&plan, "Dungeon (Disk 1 of 2).st");
    assert_eq!(st.status, sure(TidyReason::Extension));
    assert_eq!(
        tos(st),
        [
            rel("atarist/Dungeon (Disk 1 of 2).st"),
            rel("atarist/Dungeon (Disk 2 of 2).st")
        ]
    );
    assert_eq!(unit(&plan, "Worms.adf").system.as_deref(), Some("amiga"));

    let keen = unit(&plan, "Jeux/Commander Keen");
    assert_eq!(keen.status, TidyStatus::Suggested(TidyReason::DosFolder));
    assert_eq!(
        keen.whole_dir.as_ref().map(|w| &w.to),
        Some(&rel("dos/Commander Keen"))
    );
    assert!(tos(keen).contains(&rel("dos/Commander Keen/data/LEVEL1.DAT")));
    assert_eq!(keen.extension, "");

    // A headerless `.bin` (a BIOS dropped here) is only suggested, never moved by default.
    assert_eq!(
        unit(&plan, "scph5501.bin").status,
        TidyStatus::Suggested(TidyReason::Extension)
    );

    let vagrant = unit(&plan, "Vagrant.chd");
    assert_eq!(vagrant.status, TidyStatus::Ambiguous);
    assert_eq!(vagrant.candidates, ["megacd", "dos", "psx", "3do"]);

    #[cfg(unix)]
    assert_eq!(
        unit(&plan, "link.sfc").status,
        TidyStatus::Blocked(TidyBlock::Symlink)
    );

    assert!(plan.ignored_paths.contains(&rel("kick13.rom")));
    assert!(plan.ignored_paths.contains(&rel("notes.txt")));
    assert_eq!(plan.ignored, plan.ignored_paths.len() as u64);
    assert_eq!(plan.already_tidy, 1);
    // Every file belongs to one unit at most, every path is plain and relative.
    let mut seen = std::collections::HashSet::new();
    for m in &plan.meta {
        for file in &m.files {
            assert!(
                seen.insert(file.path.clone()),
                "{} in two units",
                file.path.display()
            );
        }
    }
    for u in &plan.units {
        for mv in u.moves.iter().chain(&u.whole_dir) {
            assert!(plan::is_plain_relative(&mv.from) && plan::is_plain_relative(&mv.to));
        }
    }
    let ids: Vec<u32> = plan.units.iter().map(|u| u.id).collect();
    assert_eq!(ids, (0..plan.units.len() as u32).collect::<Vec<_>>());
}

#[test]
fn choosing_a_system_fills_the_moves_and_rechecks() {
    let f = fixture();
    write(&f.roms.join("Vagrant.chd"), &noise(3000, 1));
    write(&f.roms.join("Vagrant.sbi"), &noise(30, 2));
    write(&f.roms.join("Other.chd"), &noise(3000, 3));
    write(&f.roms.join("Pocket.gb"), &noise(100, 4));
    let library = Library::open_in_memory().unwrap();
    let mut plan = f.plan(&library);
    let vagrant = unit(&plan, "Vagrant.chd").id;
    let pocket = unit(&plan, "Pocket.gb").id;

    assert!(matches!(
        plan.choose_system(vagrant, "snes"),
        Err(LibraryError::TidyChoice { .. })
    ));
    assert!(plan.choose_system(999, "psx").is_err());
    plan.choose_system(vagrant, "psx").unwrap();
    let v = unit(&plan, "Vagrant.chd");
    assert_eq!(v.status, TidyStatus::Ambiguous);
    assert_eq!(v.system.as_deref(), Some("psx"));
    assert_eq!(tos(v), [rel("psx/Vagrant.chd"), rel("psx/Vagrant.sbi")]);
    assert_eq!(v.system_change, None);

    // Bulk choice: only the still unresolved units of that extension.
    assert_eq!(plan.choose_system_for_extension(".CHD", "3do"), 1);
    assert_eq!(unit(&plan, "Other.chd").system.as_deref(), Some("3do"));
    assert_eq!(unit(&plan, "Vagrant.chd").system.as_deref(), Some("psx"));
    assert_eq!(plan.choose_system_for_extension("gb", "snes"), 0);
    plan.choose_system(pocket, "gbc").unwrap();
    assert_eq!(tos(unit(&plan, "Pocket.gb")), [rel("gbc/Pocket.gb")]);
    assert_eq!(
        plan.fingerprints(vagrant).unwrap().len(),
        2,
        "the .sbi follows its image"
    );
    assert_eq!(plan.roms_root(), f.options().roms_root());
}

#[test]
fn a_known_game_keeps_its_system_and_a_change_is_flagged() {
    let f = fixture();
    write(&f.roms.join("Known.gb"), &noise(100, 1));
    let library = Library::open_in_memory().unwrap();
    insert_game(&library, &f.roms.join("Known.gb"), "gb", "Known");
    let mut plan = f.plan(&library);
    let id = unit(&plan, "Known.gb").id;
    plan.choose_system(id, "gbc").unwrap();
    let u = unit(&plan, "Known.gb");
    assert_eq!(u.status, TidyStatus::Sure(TidyReason::Library));
    assert_eq!(u.system_change, Some(("gb".to_owned(), "gbc".to_owned())));
    assert_eq!(tos(u), [rel("gbc/Known.gb")]);
    plan.choose_system(id, "gb").unwrap();
    assert_eq!(unit(&plan, "Known.gb").system_change, None);
}

#[test]
fn cue_references_are_checked() {
    let f = fixture();
    write(
        &f.roms.join("a/Missing.cue"),
        &cue(&["Missing (Track 1).bin"]),
    );
    write(&f.roms.join("b/Abs.cue"), b"FILE \"/etc/passwd\" BINARY\n");
    write(&f.roms.join("c/Up.cue"), b"FILE \"../x.bin\" BINARY\n");
    write(&f.roms.join("d/One.cue"), &cue(&["shared.bin"]));
    write(&f.roms.join("d/Two.cue"), &cue(&["shared.bin"]));
    write(&f.roms.join("d/shared.bin"), &psx_track());
    // Two games whose tracks have the same name, each in its own folder: no sharing.
    write(&f.roms.join("e/G1.cue"), &cue(&["track.bin"]));
    write(&f.roms.join("e/track.bin"), &psx_track());
    write(&f.roms.join("g/G2.cue"), &cue(&["track.bin"]));
    write(&f.roms.join("g/track.bin"), &psx_track());
    let library = Library::open_in_memory().unwrap();
    let plan = f.plan(&library);
    assert_eq!(
        unit(&plan, "a/Missing.cue").status,
        TidyStatus::Blocked(TidyBlock::MissingReference {
            name: "Missing (Track 1).bin".into()
        })
    );
    assert_eq!(
        unit(&plan, "b/Abs.cue").status,
        TidyStatus::Blocked(TidyBlock::OutsideReference {
            name: "/etc/passwd".into()
        })
    );
    assert_eq!(
        unit(&plan, "c/Up.cue").status,
        TidyStatus::Blocked(TidyBlock::OutsideReference {
            name: "../x.bin".into()
        })
    );
    let shared = TidyStatus::Blocked(TidyBlock::SharedTrack {
        name: "shared.bin".into(),
    });
    assert_eq!(unit(&plan, "d/One.cue").status, shared);
    assert_eq!(unit(&plan, "d/Two.cue").status, shared);
    let g1 = unit(&plan, "e");
    assert_eq!(g1.whole_dir.as_ref().unwrap().to, rel("psx/e"));
    assert_eq!(
        unit(&plan, "g").whole_dir.as_ref().unwrap().to,
        rel("psx/g")
    );
}

#[test]
fn playlists_and_tracks_without_cue() {
    let f = fixture();
    write(
        &f.roms.join("Epic.m3u"),
        b"Epic/Epic (Disc 1).cue\nEpic/Epic (Disc 2).cue\n",
    );
    cd(&f.roms.join("Epic"), "Epic (Disc 1)", 0, psx_track);
    cd(&f.roms.join("Epic"), "Epic (Disc 2)", 0, psx_track);
    write(&f.roms.join("Lone (Track 1).bin"), &psx_track());
    write(&f.roms.join("Lone (Track 2).bin"), &[0u8; 4096]);
    write(&f.roms.join("Orphan (Track 2).bin"), &[0u8; 4096]);
    write(&f.roms.join("Bad.m3u"), b"../elsewhere.cue\n");
    let library = Library::open_in_memory().unwrap();
    let plan = f.plan(&library);
    let epic = unit(&plan, "Epic.m3u");
    assert_eq!(epic.system.as_deref(), Some("psx"));
    assert_eq!(epic.moves.len(), 5);
    assert!(tos(epic).contains(&rel("psx/Epic/Epic.m3u")));
    assert!(tos(epic).contains(&rel("psx/Epic/Epic/Epic (Disc 2) (Track 1).bin")));
    let lone = unit(&plan, "Lone (Track 1).bin");
    assert_eq!(lone.system.as_deref(), Some("psx"));
    assert_eq!(
        tos(lone),
        [
            rel("psx/Lone/Lone (Track 1).bin"),
            rel("psx/Lone/Lone (Track 2).bin")
        ]
    );
    assert_eq!(
        unit(&plan, "Orphan (Track 2).bin").status,
        TidyStatus::Blocked(TidyBlock::UngroupedTracks)
    );
    assert_eq!(
        unit(&plan, "Bad.m3u").status,
        TidyStatus::Blocked(TidyBlock::OutsideReference {
            name: "../elsewhere.cue".into()
        })
    );
}

#[test]
fn collisions_and_duplicates_ignore_case() {
    let f = fixture();
    let rom = noise(4096, 1);
    write(&f.roms.join("Same.sfc"), &rom);
    write(&f.roms.join("snes/SAME.sfc"), &rom);
    write(&f.roms.join("Other.sfc"), &noise(4096, 2));
    write(&f.roms.join("snes/other.SFC"), &noise(4096, 3));
    write(&f.roms.join("a/Twice.nes"), &noise(64, 4));
    write(&f.roms.join("b/TWICE.nes"), &noise(64, 5));
    write(&f.roms.join("c/Copy.nes"), &noise(64, 6));
    write(&f.roms.join("d/Copy.nes"), &noise(64, 6));
    let library = Library::open_in_memory().unwrap();
    let plan = f.plan(&library);
    assert_eq!(
        unit(&plan, "Same.sfc").status,
        TidyStatus::Blocked(TidyBlock::Duplicate {
            existing: rel("snes/SAME.sfc")
        })
    );
    assert_eq!(
        unit(&plan, "Other.sfc").status,
        TidyStatus::Blocked(TidyBlock::Conflict {
            existing: rel("snes/other.SFC")
        })
    );
    assert_eq!(
        unit(&plan, "a/Twice.nes").status,
        TidyStatus::Sure(TidyReason::Extension)
    );
    assert_eq!(
        unit(&plan, "b/TWICE.nes").status,
        TidyStatus::Blocked(TidyBlock::Conflict {
            existing: rel("nes/TWICE.nes")
        })
    );
    assert!(matches!(
        unit(&plan, "d/Copy.nes").status,
        TidyStatus::Blocked(TidyBlock::Duplicate { .. })
    ));
}

#[test]
fn a_folder_named_like_a_system_is_never_created() {
    let f = fixture();
    cd(&f.roms.join("x/gb"), "Game", 0, psx_track);
    let library = Library::open_in_memory().unwrap();
    let plan = f.plan(&library);
    assert_eq!(
        unit(&plan, "x/gb").status,
        TidyStatus::Blocked(TidyBlock::FolderNamedLikeSystem { name: "gb".into() })
    );
}

#[cfg(unix)]
#[test]
fn links_and_non_utf8_names_are_blocked() {
    use std::os::unix::ffi::OsStrExt;
    let f = fixture();
    let elsewhere = f._tmp.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, f.roms.join("psx")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, f.roms.join("linked dir")).unwrap();
    write(&f.roms.join("Lone (Track 1).bin"), &psx_track());
    let name = std::ffi::OsStr::from_bytes(b"caf\xe9.sfc");
    // APFS (macOS) refuses names that are not valid UTF-8: only check them where they can exist.
    let non_utf8 = fs::write(f.roms.join(name), noise(64, 1)).is_ok();
    let library = Library::open_in_memory().unwrap();
    let plan = f.plan(&library);
    assert_eq!(
        unit(&plan, "Lone (Track 1).bin").status,
        TidyStatus::Blocked(TidyBlock::DestinationNotPlainDir { dir: rel("psx") })
    );
    assert_eq!(
        unit(&plan, "linked dir").status,
        TidyStatus::Blocked(TidyBlock::Symlink)
    );
    assert!(
        !non_utf8
            || plan
                .units
                .iter()
                .any(|u| u.status == TidyStatus::Blocked(TidyBlock::NonUtf8Path))
    );
    assert_eq!(plan.already_tidy, 0, "a linked roms/psx is not walked");
}

#[test]
fn a_game_folder_is_one_unit_even_with_discs_inside() {
    let f = fixture();
    write(&f.roms.join("Quake/QUAKE.EXE"), &noise(500, 1));
    cd(&f.roms.join("Quake"), "Quake CD", 1, psx_track);
    write(&f.roms.join("Quake/README.TXT"), b"x");
    let library = Library::open_in_memory().unwrap();
    let plan = f.plan(&library);
    assert_eq!(plan.units.len(), 1);
    let quake = &plan.units[0];
    assert_eq!(quake.status, TidyStatus::Suggested(TidyReason::DosFolder));
    assert_eq!(quake.moves.len(), 5);
    assert_eq!(plan.meta[0].files.len(), 5);
}

#[test]
fn cancellation_stops_the_plan() {
    let f = fixture();
    write(&f.roms.join("a.sfc"), &noise(64, 1));
    let library = Library::open_in_memory().unwrap();
    let cancelled = library.plan_tidy(&f.options(), &AtomicBool::new(true), &mut |_| {});
    assert!(matches!(cancelled, Err(LibraryError::Cancelled)));
}

#[test]
fn system_folders_are_created_once() {
    let f = fixture();
    write(&f.roms.join("snes"), b"a file named like a system");
    let created = create_system_folders(&f.roms).unwrap();
    assert_eq!(created.len(), crate::systems().len() - 1);
    assert!(f.roms.join("psx").is_dir());
    assert!(f.roms.join("snes").is_file());
    assert!(create_system_folders(&f.roms).unwrap().is_empty());
    let fresh = f._tmp.path().join("new/roms");
    assert_eq!(
        create_system_folders(&fresh).unwrap().len(),
        crate::systems().len()
    );
}

#[test]
fn last_tidy_reads_the_newest_journal() {
    use super::journal::{JournalWriter, Op};
    use super::sys::RealSys;
    let f = fixture();
    let options = f.options();
    assert_eq!(last_tidy(&options).unwrap(), None);
    fs::create_dir_all(&f.journals).unwrap();
    let mut old = JournalWriter::create(&RealSys, &f.journals, options.roms_root(), 10).unwrap();
    old.append(&RealSys, &[Op::End]).unwrap();
    let mut new = JournalWriter::create(&RealSys, &f.journals, options.roms_root(), 20).unwrap();
    let mv = |from: &str, to: &str| Op::Move {
        n: 0,
        size: 1,
        mtime: 2,
        from: rel(from),
        to: rel(to),
    };
    new.append(
        &RealSys,
        &[
            Op::Begin(0),
            mv("a.sfc", "snes/a.sfc"),
            Op::MoveDone(0),
            Op::Commit(0),
            Op::Begin(1),
            mv("b.sfc", "snes/b.sfc"),
            Op::MoveDone(0),
        ],
    )
    .unwrap();
    // An empty newer file (cut header) means « nothing moved »: skipped.
    fs::write(f.journals.join("journal-30.txt"), b"emu-rust tidy").unwrap();
    let info = last_tidy(&options).unwrap().unwrap();
    assert_eq!(info.path, new.path());
    assert_eq!(info.created_unix, 20);
    assert_eq!(info.moved, 1, "an uncommitted unit does not count");
    assert!(!info.complete && !info.undone && !info.identities_done && !info.can_undo());
    fs::write(new.path(), b"garbage").unwrap();
    assert!(matches!(
        last_tidy(&options),
        Err(LibraryError::Journal { .. })
    ));
}

// ---------------------------------------------------------------------------------------------------------------
// relocate

fn scan(library: &mut Library, root: &Path) -> crate::ScanReport {
    let options = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    library
        .scan(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn by_title<'g>(games: &'g [crate::Game], title: &str) -> &'g crate::Game {
    games
        .iter()
        .find(|g| g.title == title)
        .unwrap_or_else(|| panic!("{title}: {games:#?}"))
}

fn pinned(library: &Library, id: GameId) -> bool {
    library
        .conn
        .query_row("SELECT pinned FROM game WHERE id = ?1", [id.0], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
        != 0
}

#[test]
fn relocate_keeps_the_row_and_builds_keys_like_the_scan() {
    let f = fixture();
    write(&f.roms.join("Hero.sfc"), &noise(4096, 1));
    write(&f.roms.join("Quest (Disk 1 of 2).st"), &noise(900, 2));
    write(&f.roms.join("Quest (Disk 2 of 2).st"), &noise(900, 3));
    let mut library = Library::open_in_memory().unwrap();
    // The root spelled with `.` and a trailing separator, as a user may type it.
    let spelled = f.roms.join(".").join("");
    scan(&mut library, &spelled);
    let games = library.games().unwrap();
    let hero = by_title(&games, "Hero").id;
    let quest = by_title(&games, "Quest").id;
    library.set_favorite(hero, true).unwrap();
    library.record_played(hero, 1234).unwrap();
    library
        .set_cover(
            hero,
            CoverKind::Boxart,
            &CoverState::Missing { checked_unix: 1 },
        )
        .unwrap();

    write(&f.roms.join("snes/Hero.sfc"), &noise(4096, 1));
    let hero_mtime = fs::metadata(f.roms.join("Hero.sfc"))
        .unwrap()
        .modified()
        .unwrap();
    fs::remove_file(f.roms.join("Hero.sfc")).unwrap();
    fs::File::options()
        .write(true)
        .open(f.roms.join("snes/Hero.sfc"))
        .unwrap()
        .set_modified(hero_mtime)
        .unwrap();
    fs::create_dir(f.roms.join("atarist")).unwrap();
    for n in 1..=2 {
        let name = format!("Quest (Disk {n} of 2).st");
        fs::rename(f.roms.join(&name), f.roms.join("atarist").join(&name)).unwrap();
    }
    let moves = vec![
        (rel("Hero.sfc"), rel("snes/Hero.sfc")),
        (
            rel("Quest (Disk 1 of 2).st"),
            rel("atarist/Quest (Disk 1 of 2).st"),
        ),
        (
            rel("Quest (Disk 2 of 2).st"),
            rel("atarist/Quest (Disk 2 of 2).st"),
        ),
    ];
    assert_eq!(library.relocate(&spelled, &moves).unwrap(), 2);
    let games = library.games().unwrap();
    let h = by_title(&games, "Hero");
    assert_eq!(h.id, hero);
    assert!(h.favorite);
    assert_eq!(h.play_count, 1);
    assert_eq!(h.path, f.roms.join("snes").join("Hero.sfc"));
    assert!(pinned(&library, hero));
    assert!(library.cover(hero, CoverKind::Boxart).unwrap().is_some());
    let q = by_title(&games, "Quest");
    assert_eq!(q.id, quest);
    assert_eq!(
        q.media,
        [
            f.roms.join("atarist").join("Quest (Disk 1 of 2).st"),
            f.roms.join("atarist").join("Quest (Disk 2 of 2).st")
        ]
    );
    // The next scan finds both rows unchanged: same keys, same pin.
    let report = scan(&mut library, &f.roms);
    assert_eq!((report.unchanged, report.added, report.removed), (2, 0, 0));
    assert_eq!(library.games().unwrap().len(), 2);
}

#[test]
fn relocate_follows_a_moved_folder_and_handles_rows_at_the_destination() {
    let f = fixture();
    write(&f.roms.join("Keen/KEEN.EXE"), &noise(100, 1));
    let mut library = Library::open_in_memory().unwrap();
    let keen = insert_game(&library, &f.roms.join("Keen"), "dos", "Keen");
    // A stale row where the folder lands (its file is gone).
    let stale = insert_game(&library, &f.roms.join("dos").join("Keen"), "dos", "Old");
    fs::create_dir(f.roms.join("dos")).unwrap();
    fs::rename(f.roms.join("Keen"), f.roms.join("dos/Keen")).unwrap();
    assert_eq!(
        library
            .relocate(&f.roms, &[(rel("Keen"), rel("dos/Keen"))])
            .unwrap(),
        1
    );
    assert_eq!(library.game(stale).unwrap(), None);
    let moved = library.game(keen).unwrap().unwrap();
    assert_eq!(moved.path, f.roms.join("dos").join("Keen"));
    assert!(pinned(&library, keen));

    // Both the source and the destination exist: the call does not match the disk.
    write(&f.roms.join("a.sfc"), &noise(10, 1));
    write(&f.roms.join("snes/a.sfc"), &noise(10, 2));
    let a = insert_game(&library, &f.roms.join("a.sfc"), "snes", "A");
    let live = insert_game(&library, &f.roms.join("snes/a.sfc"), "snes", "A2");
    let err = library
        .relocate(&f.roms, &[(rel("a.sfc"), rel("snes/a.sfc"))])
        .unwrap_err();
    assert!(matches!(err, LibraryError::Io { .. }));
    assert_eq!(library.game(a).unwrap().unwrap().path, f.roms.join("a.sfc"));
    assert!(library.game(live).unwrap().is_some());

    for bad in ["../x", "/abs"] {
        assert!(
            library
                .relocate(&f.roms, &[(PathBuf::from(bad), rel("snes/x"))])
                .is_err()
        );
    }
    assert_eq!(library.relocate(&f.roms, &[]).unwrap(), 0);
}

// ---------------------------------------------------------------------------------------------------------------
// moved-file net of the scan

#[test]
fn scan_recognises_a_file_moved_by_hand() {
    let f = fixture();
    write(&f.roms.join("Hero.sfc"), &noise(4096, 1));
    let mut library = Library::open_in_memory().unwrap();
    scan(&mut library, &f.roms);
    let hero = library.games().unwrap()[0].id;
    library.set_favorite(hero, true).unwrap();
    library
        .set_cover(
            hero,
            CoverKind::Boxart,
            &CoverState::Missing { checked_unix: 1 },
        )
        .unwrap();
    fs::create_dir(f.roms.join("vrac")).unwrap();
    fs::rename(f.roms.join("Hero.sfc"), f.roms.join("vrac/Hero.sfc")).unwrap();
    let report = scan(&mut library, &f.roms);
    assert_eq!((report.moved, report.added, report.removed), (1, 0, 0));
    let games = library.games().unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].id, hero);
    assert!(games[0].favorite);
    assert_eq!(games[0].path, f.roms.join("vrac/Hero.sfc"));
    assert!(library.cover(hero, CoverKind::Boxart).unwrap().is_some());
}

#[test]
fn scan_does_not_merge_two_twins_nor_a_kept_row() {
    let f = fixture();
    let rom = noise(4096, 1);
    write(&f.roms.join("Hero.sfc"), &rom);
    let mut library = Library::open_in_memory().unwrap();
    scan(&mut library, &f.roms);
    let hero = library.games().unwrap()[0].id;
    fs::remove_file(f.roms.join("Hero.sfc")).unwrap();
    write(&f.roms.join("a/Hero.sfc"), &rom);
    write(&f.roms.join("b/Hero.sfc"), &rom);
    let report = scan(&mut library, &f.roms);
    assert_eq!((report.moved, report.added, report.removed), (0, 2, 1));
    assert!(library.game(hero).unwrap().is_none());

    // A row of an absent root is kept (`keep`), never taken for a moved file.
    let ext = fixture();
    write(&ext.roms.join("Other.sfc"), &noise(4096, 9));
    let mut library = Library::open_in_memory().unwrap();
    let options = ScanOptions {
        roots: vec![f.roms.clone(), ext.roms.clone()],
        ..Default::default()
    };
    library
        .scan(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let other = by_title(&library.games().unwrap(), "Other").id;
    let unplugged = ext.roms.with_file_name("unplugged");
    fs::rename(&ext.roms, &unplugged).unwrap();
    write(&f.roms.join("c/Other.sfc"), &noise(4096, 9));
    let report = library
        .scan(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert_eq!(report.moved, 0);
    assert_eq!(report.missing_roots.len(), 1);
    assert!(library.game(other).unwrap().is_some());
    assert_eq!(
        library
            .games()
            .unwrap()
            .iter()
            .filter(|g| g.title == "Other")
            .count(),
        2
    );
}

#[test]
fn scan_recognises_a_moved_unhashed_disc_by_name_size_and_date() {
    let f = fixture();
    cd(&f.roms.join("psx"), "Disc", 0, psx_track);
    let mut library = Library::open_in_memory().unwrap();
    scan(&mut library, &f.roms);
    let games = library.games().unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].sha1, None);
    let id = games[0].id;
    fs::create_dir(f.roms.join("psx/Disc")).unwrap();
    for name in ["Disc.cue", "Disc (Track 1).bin"] {
        fs::rename(
            f.roms.join("psx").join(name),
            f.roms.join("psx/Disc").join(name),
        )
        .unwrap();
    }
    let report = scan(&mut library, &f.roms);
    assert_eq!(report.moved, 1);
    let games = library.games().unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].id, id);
    assert_eq!(games[0].path, f.roms.join("psx/Disc/Disc.cue"));
}

#[test]
fn files_named_like_a_known_bios_stay_in_place() {
    let f = fixture();
    write(&f.roms.join("SCPH1001.BIN"), &noise(512 * 1024, 1));
    write(&f.roms.join("x/gba_bios.bin"), &noise(16 * 1024, 2));
    write(&f.roms.join("Hero.sfc"), &noise(64, 3));
    let library = Library::open_in_memory().unwrap();
    let options = f.options().with_firmware_names([
        "scph1001.bin",
        "gba/gba_bios.bin",
        r"x\ignored\GBA_BIOS.BIN",
    ]);
    let plan = library
        .plan_tidy(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert_eq!(plan.units.len(), 1, "{:#?}", plan.units);
    assert!(plan.ignored_paths.contains(&rel("SCPH1001.BIN")));
    assert!(plan.ignored_paths.contains(&rel("x/gba_bios.bin")));
    // Without the list, a headerless `.bin` is still only suggested (never moved by default).
    let plan = f.plan(&library);
    assert_eq!(
        unit(&plan, "SCPH1001.BIN").status,
        TidyStatus::Suggested(TidyReason::Extension)
    );
}
