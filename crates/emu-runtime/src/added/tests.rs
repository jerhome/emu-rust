use super::*;
use std::fs::File;

struct Fx {
    tmp: tempfile::TempDir,
}

impl Fx {
    /// rc.1 leftovers in a 2.0 folder: `Game.st` (set) and `Extra.st` (added) in the manifest, `02 X.st` outside
    /// it, a stale `Game.m3u` (the set has one image: no playlist is current).
    fn new() -> Self {
        let fx = Self {
            tmp: tempfile::tempdir().unwrap(),
        };
        let work = fx.work();
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("Game.st"), b"game").unwrap();
        fs::write(work.join("Extra.st"), b"extra").unwrap();
        fs::write(work.join("02 X.st"), b"rc1 copy").unwrap();
        fs::write(work.join("Game.m3u"), b"Game.st\n02 X.st\n").unwrap();
        fs::write(
            work.join(MANIFEST_NAME),
            "emu-rust working copies 1\n\
             set\tGame.st\t\t4\t-\t00000001\t/roms/Game.st\n\
             added\tExtra.st\t\t5\t-\t00000002\t/roms/Extra.st\n",
        )
        .unwrap();
        fx
    }
    fn saves(&self) -> PathBuf {
        self.tmp.path().join("saves/hatari")
    }
    fn work(&self) -> PathBuf {
        self.saves().join("Game.disks")
    }
    fn names(&self) -> Vec<(String, OrphanKind)> {
        orphan_copies(&self.saves(), &self.work())
            .unwrap()
            .into_iter()
            .map(|o| (o.name, o.kind))
            .collect()
    }
}

#[test]
fn orphans_of_an_rc1_folder_are_listed() {
    let fx = Fx::new();
    let orphans = orphan_copies(&fx.saves(), &fx.work()).unwrap();
    assert_eq!(
        orphans
            .iter()
            .map(|o| (o.name.as_str(), o.kind))
            .collect::<Vec<_>>(),
        [
            ("02 X.st", OrphanKind::Copy),
            ("Game.m3u", OrphanKind::StalePlaylist)
        ]
    );
    assert_eq!(orphans[0].size, 8);
    assert!(orphans[0].modified.is_some());
}

#[test]
fn the_current_playlist_is_not_an_orphan() {
    let fx = Fx::new();
    let work = fx.saves().join("G (2).disks");
    fs::create_dir_all(&work).unwrap();
    for name in ["A.st", "B.st"] {
        fs::write(work.join(name), b"d").unwrap();
    }
    fs::write(
        work.join(MANIFEST_NAME),
        "emu-rust working copies 1\n\
         set\tA.st\t\t1\t-\t00000001\t/r/A.st\n\
         set\tB.st\t\t1\t-\t00000001\t/r/B.st\n",
    )
    .unwrap();
    // rc.2 homonym folder: `G.m3u` generated under the old name, `G (2).m3u` under the 2.1 save name.
    fs::write(work.join("G.m3u"), b"A.st\nB.st\n").unwrap();
    let list = |fx: &Fx| -> Vec<String> {
        orphan_copies(&fx.saves(), &work)
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect()
    };
    assert!(
        list(&fx).is_empty(),
        "the only matching playlist is current"
    );
    fs::write(work.join("G (2).m3u"), b"A.st\nB.st\n").unwrap();
    assert_eq!(list(&fx), ["G.m3u"]);
}

#[test]
fn removal_refusals_leave_everything_in_place() {
    let fx = Fx::new();
    let work = fx.work();
    let elsewhere = fx.tmp.path().join("x.disks");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("a.st"), b"a").unwrap();
    fs::write(fx.saves().join("Game.srm"), b"sram").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(fx.saves().join("Game.srm"), work.join("link.st")).unwrap();
    let refused = |dir: &Path, name: &str| {
        matches!(
            remove_orphan_copies(&fx.saves(), dir, &[name.to_owned()]),
            Err(RuntimeError::RemovalRefused { .. })
        )
    };
    for name in [
        "Game.st",
        "extra.st",
        MANIFEST_NAME,
        "../Game.srm",
        "..",
        "",
        "a/b",
    ] {
        assert!(refused(&work, name), "{name:?}");
    }
    #[cfg(unix)]
    assert!(refused(&work, "link.st"));
    assert!(refused(&elsewhere, "a.st"), "folder outside the saves");
    assert!(refused(&fx.saves().join("Game"), "x"), "not a .disks");
    assert!(orphan_copies(&fx.saves(), &elsewhere).is_err());
    // One bad name in the selection: nothing deleted, not even the valid ones.
    assert!(
        remove_orphan_copies(
            &fx.saves(),
            &work,
            &["02 X.st".to_owned(), "Game.st".to_owned()]
        )
        .is_err()
    );
    assert!(work.join("02 X.st").is_file());
    assert!(remove_orphan_copies(&fx.saves(), &work, &["missing.st".to_owned()]).is_err());
    assert_eq!(fs::read(fx.saves().join("Game.srm")).unwrap(), b"sram");
    assert!(elsewhere.join("a.st").is_file());

    assert_eq!(
        remove_orphan_copies(&fx.saves(), &work, &["02 X.st".to_owned()]).unwrap(),
        8
    );
    assert!(!work.join("02 X.st").exists());
    assert!(work.join("Game.m3u").is_file(), "unticked orphan kept");
    assert!(work.join("Game.st").is_file() && work.join("Extra.st").is_file());
}

#[cfg(unix)]
#[test]
fn a_linked_folder_is_never_listed_nor_emptied() {
    let fx = Fx::new();
    let target = fx.tmp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("a.st"), b"a").unwrap();
    let link = fx.saves().join("L.disks");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(orphan_copies(&fx.saves(), &link).is_err());
    assert!(remove_orphan_copies(&fx.saves(), &link, &["a.st".to_owned()]).is_err());
    assert!(target.join("a.st").is_file());
}

#[test]
fn folders_without_a_readable_manifest_list_nothing() {
    let fx = Fx::new();
    let old = fx.saves().join("Old.disks");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("Old.st"), b"o").unwrap();
    assert!(orphan_copies(&fx.saves(), &old).unwrap().is_empty());
    assert!(remove_orphan_copies(&fx.saves(), &old, &["Old.st".to_owned()]).is_err());
    fs::write(fx.work().join(MANIFEST_NAME), b"damaged").unwrap();
    assert!(fx.names().is_empty());
    assert!(
        orphan_copies(&fx.saves(), &fx.saves().join("Absent.disks"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn only_old_temporary_files_go_automatically() {
    let fx = Fx::new();
    let work = fx.work();
    let old = work.join(".Game.st.tmp-123");
    let recent = work.join(".Extra.st.tmp-456");
    fs::write(&old, b"partial").unwrap();
    fs::write(&recent, b"partial").unwrap();
    let two_hours_ago = SystemTime::now() - Duration::from_secs(7200);
    File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(two_hours_ago)
        .unwrap();
    // Never listed (nor removable through the dialog).
    let names = fx.names();
    assert!(names.iter().all(|(n, _)| !n.contains(".tmp-")), "{names:?}");
    assert!(!old.exists());
    assert!(recent.exists());
    assert!(remove_orphan_copies(&fx.saves(), &work, &[".Extra.st.tmp-456".to_owned()]).is_err());
    assert!(recent.exists());
    // The rest of the folder is untouched by the listing.
    assert!(work.join("02 X.st").is_file() && work.join("Game.m3u").is_file());
}
