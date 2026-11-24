//! Schema v3 (2.0) -> v4 (3.0) on a base written by the real 2.0.0-rc.2 code (`tests/data/library/`, see the
//! README there): nothing is lost, the DAT sources are assigned, ST and consoles identify as before.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use emu_library::{
    CoverKind, CoverState, Dat, DatEntry, DatSource, GameId, Identification, Library, OpenOutcome,
    SCHEMA_VERSION, ScanOptions,
};
use rusqlite::Connection;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/library/library-2.0.sqlite")
}

fn user_version(db: &Path) -> i64 {
    Connection::open(db)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

/// All rows of `sql`, rendered as text (SQLite values only: integers, text, blobs as hex, NULL).
fn rows(db: &Path, sql: &str) -> Vec<String> {
    let conn = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        let mut cols = Vec::new();
        for i in 0..n {
            cols.push(match r.get_ref(i).unwrap() {
                rusqlite::types::ValueRef::Null => "NULL".to_owned(),
                rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                rusqlite::types::ValueRef::Real(v) => v.to_string(),
                rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                rusqlite::types::ValueRef::Blob(b) => {
                    b.iter().map(|x| format!("{x:02x}")).collect()
                }
            });
        }
        Ok(cols.join("|"))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

// The v1-v3 columns: `is_dir` (v5) is checked by `migration_v5.rs`.
const GAME_SQL: &str = "SELECT id, path, entry, system, size, file_size, mtime_unix, crc32, sha1, crc32_nh, \
                        sha1_nh, title, identification, favorite, last_played_unix, play_count, pinned \
                        FROM game ORDER BY id";
const MEDIA_SQL: &str = "SELECT * FROM game_media ORDER BY game_id, idx";
const COVER_SQL: &str = "SELECT * FROM cover ORDER BY game, kind";

#[test]
fn a_real_2_0_base_migrates_to_the_current_schema_without_loss() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite");
    fs::copy(fixture(), &db).unwrap();
    assert_eq!(user_version(&db), 3, "the fixture is a v3 base");
    let games = rows(&db, GAME_SQL);
    let media = rows(&db, MEDIA_SQL);
    let covers = rows(&db, COVER_SQL);
    let dat_before = rows(
        &db,
        "SELECT system, name, version, entries, imported_unix, etag FROM dat_info ORDER BY system",
    );
    let entries_before = rows(
        &db,
        "SELECT system, game, rom_name, size, crc32, sha1 FROM dat_entry ORDER BY rowid",
    );
    assert_eq!(games.len(), 7);
    assert_eq!(dat_before.len(), 3);

    let (lib, outcome) = Library::open(&db).unwrap();
    assert_eq!(outcome, OpenOutcome::Opened);
    assert_eq!(SCHEMA_VERSION, 7);
    drop(lib);
    assert_eq!(user_version(&db), 7);
    // Games (identification, favorites, history, pins), disks and covers are byte-identical rows.
    assert_eq!(rows(&db, GAME_SQL), games);
    assert_eq!(rows(&db, MEDIA_SQL), media);
    assert_eq!(rows(&db, COVER_SQL), covers);
    // DAT entries keep their order and content; each system got the source it had in 2.0.
    assert_eq!(
        rows(
            &db,
            "SELECT system, game, rom_name, size, crc32, sha1 FROM dat_entry ORDER BY rowid"
        ),
        entries_before
    );
    let sources = rows(&db, "SELECT system, source FROM dat_info ORDER BY system");
    assert_eq!(
        sources,
        ["atarist|tosec", "megadrive|no-intro", "snes|no-intro"]
    );
    let dat_after = rows(
        &db,
        "SELECT system, name, version, entries, imported_unix, etag FROM dat_info ORDER BY system",
    );
    // Only the ST ETag changes (2.0 fetched that file from No-Intro: dropped so that the next update fetches the
    // TOSEC one, documented in the migration).
    let strip = |v: &[String]| -> Vec<String> {
        v.iter()
            .filter(|l| !l.starts_with("atarist|"))
            .cloned()
            .collect()
    };
    assert_eq!(strip(&dat_after), strip(&dat_before));
    assert!(
        dat_after
            .iter()
            .any(|l| l.starts_with("atarist|") && l.ends_with("|NULL"))
    );

    // The API sees the same library as the 2.0 code did.
    let lib = Library::open_existing(&db).unwrap();
    let all = lib.games().unwrap();
    assert_eq!(all.len(), 7);
    let g = |id: i64| all.iter().find(|g| g.id == GameId(id)).unwrap();
    assert_eq!(g(1).system, "atarist");
    assert_eq!(g(1).media.len(), 2);
    assert!(g(1).favorite && g(4).favorite);
    assert_eq!(
        (g(1).play_count, g(1).last_played_unix),
        (2, Some(1_700_000_100))
    );
    assert_eq!(
        (g(6).play_count, g(6).last_played_unix),
        (1, Some(1_700_000_500))
    );
    assert_eq!(g(2).identification, Identification::Crc32Size);
    assert_eq!(g(5).identification, Identification::Unknown);
    assert_eq!(g(2).title, "Solo (1990)(Pub)");
    assert_eq!(
        lib.cover(GameId(1), CoverKind::Boxart)
            .unwrap()
            .unwrap()
            .state,
        CoverState::Cached {
            path: "snes/boxart/Alpha (USA).png".into(),
            source: "libretro".into()
        }
    );
    assert!(matches!(
        lib.cover(GameId(2), CoverKind::Boxart)
            .unwrap()
            .unwrap()
            .state,
        CoverState::Missing {
            checked_unix: 1_700_000_900
        }
    ));
    let info = lib.dat_info("snes").unwrap().unwrap();
    assert_eq!(
        (info.source, info.entries, info.version.as_deref()),
        (DatSource::NoIntro, 2, Some("20251101"))
    );
    assert_eq!(info.etag.as_deref(), Some("\"snes-etag\""));
    assert_eq!(
        lib.dat_info("atarist").unwrap().unwrap().source,
        DatSource::Tosec
    );
    drop(lib);

    // A second opening is a no-op; the file stays v4 and unchanged in content.
    let (_, outcome) = Library::open(&db).unwrap();
    assert_eq!(outcome, OpenOutcome::Opened);
    assert_eq!(rows(&db, GAME_SQL), games);
}

#[test]
fn after_migration_a_scan_of_a_missing_root_keeps_the_games_and_dats_are_per_source() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite");
    fs::copy(fixture(), &db).unwrap();
    let (mut lib, _) = Library::open(&db).unwrap();
    // `/r` does not exist (unmounted drive): games are kept, not removed.
    let report = lib
        .scan(
            &ScanOptions {
                roots: vec![PathBuf::from("/r")],
                ..Default::default()
            },
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(lib.games().unwrap().len(), 7);

    // 3.0 allows a second DAT for Amiga without touching the migrated ST / console DATs.
    let entry = DatEntry {
        game: "Some Game".into(),
        rom_name: "g.adf".into(),
        size: 1,
        crc32: 1,
        sha1: None,
        serial: None,
        region: None,
    };
    lib.import_dat_source(
        "amiga",
        DatSource::Tosec,
        &Dat {
            name: "Commodore Amiga - Games".into(),
            version: None,
            entries: vec![entry],
        },
        None,
    )
    .unwrap();
    drop(lib);
    assert_eq!(
        rows(
            &db,
            "SELECT system, source, entries FROM dat_info ORDER BY system, source"
        ),
        [
            "amiga|tosec|1",
            "atarist|tosec|1",
            "megadrive|no-intro|2",
            "snes|no-intro|2"
        ]
    );
    assert_eq!(rows(&db, GAME_SQL).len(), 7);
}

/// ST and consoles identify exactly as the 2.0 code did: the same files, scanned and identified by the 3.0 code on
/// a fresh base, give the same system, title and identification as the migrated 2.0 rows.
#[test]
fn st_and_consoles_identify_as_in_2_0() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("roms");
    let mut snes = Vec::new();
    let mut md = Vec::new();
    let mut st = Vec::new();
    let put = |dir: &str, name: &str, data: &[u8], dat: Option<&mut Vec<DatEntry>>| {
        let d = roms.join(dir);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(name), data).unwrap();
        if let Some(dat) = dat {
            dat.push(DatEntry {
                game: name.rsplit_once('.').unwrap().0.to_owned(),
                rom_name: name.to_owned(),
                size: data.len() as u64,
                crc32: crc32(data),
                sha1: None,
                serial: None,
                region: None,
            });
        }
    };
    for (i, n) in ["Alpha (USA).sfc", "Beta (Europe).sfc"].iter().enumerate() {
        put("snes", n, &vec![0x10 + i as u8; 1024 + i], Some(&mut snes));
    }
    for (i, n) in ["Gamma (World).md", "Delta (World).md", "Unlisted.md"]
        .iter()
        .enumerate()
    {
        let dat = (i < 2).then_some(&mut md);
        put("megadrive", n, &vec![0x20 + i as u8; 2048 + i], dat);
    }
    for (i, n) in [
        "Saga (Disk 1 of 2).st",
        "Saga (Disk 2 of 2).st",
        "Solo (1990)(Pub).st",
    ]
    .iter()
    .enumerate()
    {
        let dat = (i == 2).then_some(&mut st);
        put("atarist", n, &vec![0x30 + i as u8; 4096 + i], dat);
    }
    let mut fresh = Library::open_in_memory().unwrap();
    fresh
        .scan(
            &ScanOptions {
                roots: vec![roms],
                ..Default::default()
            },
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    for (system, entries) in [("snes", snes), ("megadrive", md), ("atarist", st)] {
        let dat = Dat {
            name: system.into(),
            version: None,
            entries,
        };
        fresh.import_dat(system, &dat, None).unwrap();
    }
    let view = |lib: &Library| {
        let mut v: Vec<_> = lib
            .games()
            .unwrap()
            .into_iter()
            .map(|g| {
                (
                    g.path.file_name().unwrap().to_string_lossy().into_owned(),
                    g.system,
                    g.title,
                    g.identification,
                    g.media.len(),
                )
            })
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    let migrated = Library::open(&{
        let db = tmp.path().join("library.sqlite");
        fs::copy(fixture(), &db).unwrap();
        db
    })
    .unwrap()
    .0;
    let (a, b) = (view(&migrated), view(&fresh));
    assert_eq!(a.len(), 7);
    // DAT game names differ in the fixture only by the DAT title; compare systems, kinds and disks, and the
    // titles of the identified games, which come from the same entries.
    assert_eq!(a, b);
}

fn crc32(data: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(data);
    h.finalize()
}
