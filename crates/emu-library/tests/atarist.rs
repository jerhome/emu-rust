//! Atari ST scan: multi-disk groups, `.m3u`, zips (generated fixtures, no ROM, no network).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use emu_library::{
    Dat, DatEntry, Game, Identification, Library, ScanOptions, ScanReport, hash_reader,
};

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn disk(seed: u8) -> Vec<u8> {
    (0..2048usize)
        .map(|i| (i as u8).wrapping_mul(7) ^ seed)
        .collect()
}

fn scan(lib: &mut Library, root: &Path) -> ScanReport {
    let opts = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    lib.scan(&opts, &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn games(lib: &Library) -> Vec<Game> {
    lib.games().unwrap()
}

fn stored_zip(name: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let crc = crc32fast::hash(data);
    let size = data.len() as u32;
    let mut common = Vec::new();
    for v in [20u16, 0, 0] {
        common.extend_from_slice(&v.to_le_bytes());
    }
    common.extend_from_slice(&0u32.to_le_bytes());
    common.extend_from_slice(&crc.to_le_bytes());
    common.extend_from_slice(&size.to_le_bytes());
    common.extend_from_slice(&size.to_le_bytes());
    common.extend_from_slice(&(name.len() as u16).to_le_bytes());
    common.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    out.extend_from_slice(&common);
    out.extend_from_slice(name);
    out.extend_from_slice(data);
    let cd = out.len() as u32;
    let mut central = Vec::new();
    central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    central.extend_from_slice(&20u16.to_le_bytes());
    central.extend_from_slice(&common);
    central.extend_from_slice(&[0u8; 10]);
    central.extend_from_slice(&0u32.to_le_bytes());
    central.extend_from_slice(name);
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

fn names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn disk_groups_become_one_game_with_the_ordered_disks() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms").join("atarist");
    write(&root.join("Quest (1990)(Pub)(Disk 2 of 3).st"), &disk(2));
    write(&root.join("Quest (1990)(Pub)(Disk 1 of 3).st"), &disk(1));
    write(&root.join("Quest (1990)(Pub)(Disk 3 of 3).st"), &disk(3));
    write(&root.join("Solo (1991)(Pub).st"), &disk(9));
    // The first disk is identified by the DAT; the game takes the title without the disk marker.
    let first = disk(1);
    let h = hash_reader(&first[..], first.len() as u64, None, u64::MAX)
        .unwrap()
        .full;
    let mut lib = Library::open_in_memory().unwrap();
    lib.import_dat(
        "atarist",
        &Dat {
            name: "TOSEC".into(),
            version: None,
            entries: vec![DatEntry {
                game: "Quest (1990)(Pub)(Disk 1 of 3)".into(),
                rom_name: "q.st".into(),
                size: h.size,
                crc32: h.crc32,
                sha1: Some(h.sha1),
                serial: None,
                region: None,
            }],
        },
        None,
    )
    .unwrap();

    let report = scan(&mut lib, &root);
    assert_eq!((report.added, report.errors.len()), (2, 0), "{report:?}");
    let all = games(&lib);
    assert_eq!(all.len(), 2);
    let quest = all.iter().find(|g| g.title.starts_with("Quest")).unwrap();
    assert_eq!(quest.title, "Quest (1990)(Pub)");
    assert_eq!(quest.identification, Identification::Sha1);
    assert_eq!(quest.system, "atarist");
    assert_eq!(
        names(&quest.media),
        [
            "Quest (1990)(Pub)(Disk 1 of 3).st",
            "Quest (1990)(Pub)(Disk 2 of 3).st",
            "Quest (1990)(Pub)(Disk 3 of 3).st"
        ]
    );
    assert_eq!(quest.path, quest.media[0]);
    let solo = all.iter().find(|g| g.title.starts_with("Solo")).unwrap();
    assert!(solo.media.is_empty());
    assert_eq!(lib.game(quest.id).unwrap().unwrap().media, quest.media);

    // Unchanged rescan keeps the ids and the list.
    let again = scan(&mut lib, &root);
    assert_eq!((again.added, again.updated, again.unchanged), (0, 0, 2));
    assert_eq!(games(&lib), all);
}

#[test]
fn a_disk_appearing_later_regroups_without_losing_the_game() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("atarist");
    write(&root.join("Duo (Disk 1).st"), &disk(1));
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    let before = games(&lib);
    assert_eq!(before.len(), 1);
    assert!(before[0].media.is_empty());
    assert_eq!(before[0].title, "Duo (Disk 1)");
    lib.set_favorite(before[0].id, true).unwrap();

    write(&root.join("Duo (Disk 2).st"), &disk(2));
    let report = scan(&mut lib, &root);
    assert_eq!(report.updated, 1);
    let after = games(&lib);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, before[0].id);
    assert!(after[0].favorite);
    assert_eq!(after[0].title, "Duo");
    assert_eq!(
        names(&after[0].media),
        ["Duo (Disk 1).st", "Duo (Disk 2).st"]
    );

    fs::remove_file(root.join("Duo (Disk 2).st")).unwrap();
    scan(&mut lib, &root);
    let last = games(&lib);
    assert_eq!(last.len(), 1);
    assert!(last[0].media.is_empty());
}

#[test]
fn an_m3u_is_the_game_and_hides_its_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("atarist");
    write(&root.join("disks/a.st"), &disk(1));
    write(&root.join("disks/b.msa"), &disk(2));
    write(
        &root.join("Saga.m3u"),
        b"#EXTM3U\ndisks/a.st\r\ndisks\\b.msa\nmissing.st\n",
    );
    write(&root.join("Other.st"), &disk(5));
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    let all = games(&lib);
    assert_eq!(all.len(), 2, "{all:?}");
    let saga = all.iter().find(|g| g.path.ends_with("Saga.m3u")).unwrap();
    assert_eq!(names(&saga.media), ["a.st", "b.msa"]);
    assert_eq!(saga.system, "atarist");
}

#[test]
fn hostile_or_empty_m3u_is_reported_and_not_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("atarist");
    let outside = tmp.path().join("secret.st");
    write(&outside, &disk(7));
    write(&root.join("evil.m3u"), b"../../secret.st\n");
    write(
        &root.join("abs.m3u"),
        format!("{}\n", outside.display()).as_bytes(),
    );
    write(&root.join("empty.m3u"), b"# nothing\nnope.st\n");
    write(&root.join("big.m3u"), &vec![b'x'; 70 * 1024]);
    write(&root.join("ok.st"), &disk(1));
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(report.errors.len(), 3, "{report:?}");
    // An absolute path is valid (the runtime accepts it): the playlist is listed.
    let all = games(&lib);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all.iter().any(|g| g.path.ends_with("ok.st")));
    assert!(all.iter().any(|g| g.path.ends_with("abs.m3u")));
}

#[test]
fn m3u_labels_are_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("atarist");
    write(&root.join("a.st"), &disk(1));
    write(&root.join("b.st"), &disk(2));
    write(&root.join("Saga.m3u"), b"a.st|Disk A\nb.st|Disk B\n");
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert!(report.errors.is_empty(), "{report:?}");
    let all = games(&lib);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(names(&all[0].media), ["a.st", "b.st"]);
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn m3u_entries_match_files_ignoring_case() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("atarist");
    write(&root.join("disk1.st"), &disk(1));
    write(&root.join("Saga.m3u"), b"DISK1.ST\n");
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    let all = games(&lib);
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(all[0].path.ends_with("Saga.m3u"));
}

#[test]
fn zips_holding_an_st_image_are_scanned() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("atarist");
    write(
        &root.join("Zipped.zip"),
        &stored_zip(b"sub/Zipped.ST", &disk(4)),
    );
    write(&root.join("Text.zip"), &stored_zip(b"readme.txt", b"hi"));
    let mut lib = Library::open_in_memory().unwrap();
    let report = scan(&mut lib, &root);
    assert_eq!(report.errors.len(), 1, "{report:?}");
    let all = games(&lib);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].system, "atarist");
    assert_eq!(all[0].entry.as_deref(), Some("Zipped.ST"));
    assert!(all[0].media.is_empty());
    assert!(all[0].sha1.is_some());
}

#[test]
fn console_files_are_never_grouped() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    write(&root.join("Game (Disk 1).gb"), &disk(1));
    write(&root.join("Game (Disk 2).gb"), &disk(2));
    let mut lib = Library::open_in_memory().unwrap();
    scan(&mut lib, &root);
    let all = games(&lib);
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|g| g.media.is_empty()));
}
