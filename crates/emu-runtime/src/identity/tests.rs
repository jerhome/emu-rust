use super::*;
use crate::media::MediaSet;
use std::collections::BTreeMap;
use tempfile::TempDir;

struct Fx {
    tmp: TempDir,
}

impl Fx {
    fn new() -> Self {
        let fx = Self {
            tmp: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(fx.saves()).unwrap();
        fs::create_dir_all(fx.states()).unwrap();
        fx
    }
    fn saves(&self) -> PathBuf {
        self.tmp.path().join("saves/core")
    }
    fn states(&self) -> PathBuf {
        self.tmp.path().join("states/core")
    }
    fn rom(&self, rel: &str, data: &[u8]) -> PathBuf {
        let path = self.tmp.path().join("roms").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }
    fn resolve(&self, rom: &Path) -> GameIdentity {
        self.try_resolve(rom).unwrap()
    }
    fn try_resolve(&self, rom: &Path) -> Result<GameIdentity, RuntimeError> {
        let spec = ContentSpec::File(rom.to_path_buf());
        let (saves, states) = (self.saves(), self.states());
        resolve_game_identity(&identity_request_for(&spec, &saves, &states))
    }
    fn registry(&self) -> Vec<u8> {
        fs::read(self.saves().join(REGISTRY_NAME)).unwrap()
    }
    /// Every file under saves/ and states/ with its content.
    fn tree(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, root, out);
                } else {
                    out.insert(
                        p.strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(&p).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        for d in ["saves", "states"] {
            walk(&self.tmp.path().join(d), self.tmp.path(), &mut out);
        }
        out
    }
}

fn registry_files(name: &Path) -> bool {
    name.file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(REGISTRY_NAME)
}

#[test]
fn bare_name_at_first_launch_and_idempotent() {
    let fx = Fx::new();
    let rom = fx.rom("a/Game.bin", b"rom");
    let id = fx.resolve(&rom);
    assert_eq!((id.save_name.as_str(), id.index), ("Game", 1));
    assert_eq!(id.base_name(), "Game");
    assert_eq!(id.core_save_dir, fx.saves());
    assert_eq!(id.notice, None);
    assert!(!id.inherits_legacy() && !id.has_homonyms());
    assert_eq!(
        id.game_fp(),
        Some(format!("3:{:08x}", crc32fast::hash(b"rom")))
    );
    let registry = fx.registry();
    assert!(registry.starts_with(b"emu-rust games 1\nGame\tGame\t1\t0\t3\t"));
    assert_eq!(fx.resolve(&rom), id);
    assert_eq!(fx.registry(), registry, "nothing rewritten");
}

#[test]
fn existing_files_are_adopted_without_any_write_outside_the_registry() {
    let fx = Fx::new();
    let rom = fx.rom("Game.bin", b"rom");
    fs::write(fx.saves().join("Game.srm"), b"sram").unwrap();
    fs::write(fx.states().join("Game.state0"), b"state").unwrap();
    fs::create_dir_all(fx.saves().join("Game.disks")).unwrap();
    let before = fx.tree();
    let id = fx.resolve(&rom);
    assert_eq!((id.save_name.as_str(), id.index), ("Game", 1));
    let after = fx.tree();
    for (path, data) in &before {
        assert_eq!(after.get(path), Some(data), "{}", path.display());
    }
    assert!(
        after
            .keys()
            .filter(|p| !before.contains_key(*p))
            .all(|p| registry_files(p))
    );
    // Marked legacy: a later homonym inherits the shared SRAM.
    assert!(fx.registry().windows(9).any(|w| w == b"\tGame\t1\t1"));
}

#[test]
fn homonym_gets_a_suffix_and_inherits_the_legacy_sram() {
    let fx = Fx::new();
    let a = fx.rom("a/Twin.bin", b"aaaa");
    let b = fx.rom("b/Twin.bin", b"bbbb");
    fs::write(fx.saves().join("Twin.srm"), b"shared").unwrap();
    fs::write(fx.states().join("Twin.state4"), b"st").unwrap();
    let first = fx.resolve(&a);
    assert_eq!(first.save_name, "Twin");
    let second = fx.resolve(&b);
    assert_eq!((second.save_name.as_str(), second.index), ("Twin (2)", 2));
    assert_eq!(second.core_save_dir, fx.saves().join("Twin (2)"));
    assert!(
        !second.core_save_dir.exists(),
        "created by the load, not here"
    );
    assert_eq!(
        second.notice,
        Some(IdentityNotice::Homonym {
            first: fs::canonicalize(&a).unwrap(),
            sram_copied: true,
            legacy_states: true,
        })
    );
    assert!(second.inherits_legacy() && second.has_homonyms());
    assert_eq!(
        fs::read(fx.saves().join("Twin (2).srm")).unwrap(),
        b"shared"
    );
    assert_eq!(fs::read(fx.saves().join("Twin.srm")).unwrap(), b"shared");
    assert!(!fx.states().join("Twin (2).state4").exists());
    // Next launches: silent, and the first now knows it has a homonym.
    let again = fx.resolve(&b);
    assert_eq!(again.notice, None);
    assert!(!again.inherits_legacy());
    assert!(fx.resolve(&a).has_homonyms());
}

#[test]
fn sram_inherited_only_from_a_legacy_entry_and_never_over_a_file() {
    let fx = Fx::new();
    // Entry 1 created on an empty folder: not legacy; its later SRAM is its own.
    let a = fx.rom("a/G.bin", b"a");
    let b = fx.rom("b/G.bin", b"b");
    fx.resolve(&a);
    fs::write(fx.saves().join("G.srm"), b"of a").unwrap();
    let second = fx.resolve(&b);
    assert_eq!(second.save_name, "G (2)");
    assert!(!second.inherits_legacy());
    assert!(matches!(
        second.notice,
        Some(IdentityNotice::Homonym {
            sram_copied: false,
            legacy_states: false,
            ..
        })
    ));
    assert!(!fx.saves().join("G (2).srm").exists());

    // Legacy entry, but the target exists (written by a later version, restored by hand...): untouched.
    let fx = Fx::new();
    let a = fx.rom("a/H.bin", b"a");
    let b = fx.rom("b/H.bin", b"b");
    fs::write(fx.saves().join("H.srm"), b"shared").unwrap();
    fx.resolve(&a);
    // `H (2).srm` exists: rule 4 skips the name rather than capturing it.
    fs::write(fx.saves().join("H (2).srm"), b"foreign").unwrap();
    let second = fx.resolve(&b);
    assert_eq!(second.save_name, "H (3)");
    assert_eq!(fs::read(fx.saves().join("H (2).srm")).unwrap(), b"foreign");
    assert_eq!(fs::read(fx.saves().join("H (3).srm")).unwrap(), b"shared");
}

#[test]
fn suffix_skips_foreign_files_and_folders() {
    let fx = Fx::new();
    let a = fx.rom("a/G.bin", b"a");
    let b = fx.rom("b/G.bin", b"b");
    fx.resolve(&a);
    fs::write(fx.states().join("G (2).state1"), b"x").unwrap();
    fs::create_dir_all(fx.saves().join("g (3)")).unwrap();
    fs::write(fx.saves().join("G (4).disks"), b"not even a folder").unwrap();
    assert_eq!(fx.resolve(&b).save_name, "G (5)");
}

#[test]
fn same_path_with_new_content_keeps_the_identity() {
    let fx = Fx::new();
    let rom = fx.rom("Game.bin", b"v1");
    let id = fx.resolve(&rom);
    fs::write(&rom, b"patched v2").unwrap();
    let again = fx.resolve(&rom);
    assert_eq!(again.save_name, id.save_name);
    assert_ne!(again.game_fp(), id.game_fp(), "fingerprint refreshed");
    assert_eq!(
        again.game_fp(),
        Some(format!("10:{:08x}", crc32fast::hash(b"patched v2")))
    );
}

#[test]
fn moved_or_renamed_file_keeps_its_saves() {
    let fx = Fx::new();
    let rom = fx.rom("old/Game.bin", b"content");
    let other = fx.rom("x/Game.bin", b"other");
    assert_eq!(fx.resolve(&rom).save_name, "Game");
    assert_eq!(fx.resolve(&other).save_name, "Game (2)");
    // Folder renamed: same name, the recorded path is gone.
    let moved = fx.tmp.path().join("roms/new/Game.bin");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(&rom, &moved).unwrap();
    assert_eq!(fx.resolve(&moved).save_name, "Game");
    // File renamed: the saves follow, under the old name.
    let renamed = fx.tmp.path().join("roms/new/Game (Fixed).bin");
    fs::rename(&moved, &renamed).unwrap();
    let id = fx.resolve(&renamed);
    assert_eq!(
        (id.save_name.as_str(), id.base_name()),
        ("Game", "Game (Fixed)")
    );
    assert_eq!(fx.resolve(&renamed), id, "path updated");
    assert_eq!(fx.resolve(&other).save_name, "Game (2)");
}

#[test]
fn identical_copies_at_two_present_paths_are_two_games() {
    let fx = Fx::new();
    let a = fx.rom("a/Game.bin", b"same");
    let b = fx.rom("b/Game.bin", b"same");
    assert_eq!(fx.resolve(&a).save_name, "Game");
    let second = fx.resolve(&b);
    assert_eq!(second.save_name, "Game (2)");
    assert_eq!(second.game_fp(), fx.resolve(&a).game_fp());
}

/// Manifest written by rc.2 (`workcopy::encode`) for a set whose copy lives in `folder`.
fn rc2_manifest(dir: &Path, source: &Path, data: &[u8]) {
    fs::create_dir_all(dir).unwrap();
    let mut text = b"emu-rust working copies 1\nset\tGame.st\t\t".to_vec();
    text.extend_from_slice(
        format!("{}\t-\t{:08x}\t", data.len(), crc32fast::hash(data)).as_bytes(),
    );
    escape(&path_key(&fs::canonicalize(source).unwrap()), &mut text);
    text.push(b'\n');
    fs::write(dir.join(workcopy::MANIFEST_NAME), text).unwrap();
    fs::write(dir.join("Game.st"), data).unwrap();
}

fn media(name: &str, image: &Path) -> ContentSpec {
    ContentSpec::Media {
        set: MediaSet {
            name: name.into(),
            images: vec![image.to_path_buf()],
            playlist: None,
            launch: None,
        },
        rules: crate::media::MediaRules::new(crate::media::MediaPolicy::WorkingCopy),
    }
}

#[test]
fn a_2_0_working_copy_folder_gives_its_name() {
    let fx = Fx::new();
    let a = fx.rom("Jeu1/Game.st", b"disk a");
    let b = fx.rom("Jeu2/Game.st", b"disk b");
    rc2_manifest(&fx.saves().join("Game.disks"), &a, b"disk a");
    rc2_manifest(&fx.saves().join("Game (2).disks"), &b, b"disk b");
    let (saves, states) = (fx.saves(), fx.states());
    let resolve = |spec: &ContentSpec| {
        resolve_game_identity(&identity_request_for(spec, &saves, &states)).unwrap()
    };
    // The second game is launched first in 2.1: it still gets its own folder's name.
    let second = resolve(&media("Game", &b));
    assert_eq!((second.save_name.as_str(), second.index), ("Game (2)", 2));
    // No entry 1 yet: the banner still comes once, naming the owner of `Game.disks`.
    assert!(matches!(
        &second.notice,
        Some(IdentityNotice::Homonym { first, .. }) if *first == fs::canonicalize(&a).unwrap()
    ));
    let first = resolve(&media("Game", &a));
    assert_eq!((first.save_name.as_str(), first.index), ("Game", 1));

    // Matched by content when the path changed (rc.2 recorded another path).
    let fx = Fx::new();
    let elsewhere = fx.rom("old/Game.st", b"disk c");
    rc2_manifest(&fx.saves().join("Game (2).disks"), &elsewhere, b"disk c");
    fs::remove_file(&elsewhere).unwrap();
    let moved = fx.rom("new/Game.st", b"disk c");
    let (saves, states) = (fx.saves(), fx.states());
    let spec = media("Game", &moved);
    let id = resolve_game_identity(&identity_request_for(&spec, &saves, &states)).unwrap();
    assert_eq!(id.save_name, "Game (2)");
}

#[test]
fn unusable_registry_is_never_rewritten() {
    for content in [b"garbage\n".to_vec(), vec![b'x'; MAX_REGISTRY_BYTES + 1]] {
        let fx = Fx::new();
        let rom = fx.rom("Game.bin", b"rom");
        let path = fx.saves().join(REGISTRY_NAME);
        fs::write(&path, &content).unwrap();
        let id = fx.resolve(&rom);
        assert_eq!(id.save_name, "Game");
        assert_eq!(id.core_save_dir, fx.saves());
        assert_eq!(id.notice, Some(IdentityNotice::RegistryUnusable));
        assert_eq!(id.game_fp(), None);
        assert_eq!(fs::read(&path).unwrap(), content);
        let copies = || {
            fs::read_dir(fx.saves())
                .unwrap()
                .filter(|e| {
                    let e = e.as_ref().unwrap();
                    e.file_name().to_string_lossy().contains(".corrupt-")
                })
                .count()
        };
        // An oversized registry is left in place but not duplicated.
        let expected = usize::from(content.len() <= MAX_REGISTRY_BYTES);
        assert_eq!(copies(), expected);
        fx.resolve(&rom);
        assert_eq!(copies(), expected, "one copy, not one per launch");
    }
}

#[test]
fn registry_decoding_refuses_what_we_never_write() {
    let ok = "emu-rust games 1\nG (2)\tG\t2\t0\t1\t-\t00000001\t/a\n";
    assert!(decode(ok.as_bytes()).is_some());
    for bad in [
        "",
        "emu-rust games 2\n",
        "emu-rust games 1\nG (2)\tG\t3\t0\t1\t-\t00000001\t/a\n",
        "emu-rust games 1\nG\tG\t0\t0\t1\t-\t00000001\t/a\n",
        "emu-rust games 1\n../x\t../x\t1\t0\t1\t-\t00000001\t/a\n",
        "emu-rust games 1\nG\tG\t1\t2\t1\t-\t00000001\t/a\n",
        "emu-rust games 1\nG\tG\t1\t0\t1\t-\t00000001\t\n",
        "emu-rust games 1\nG\tG\t1\t0\t1\t-\t00000001\t/a\ng\tg\t1\t0\t1\t-\t00000001\t/b\n",
        "emu-rust games 1\nG\tG\t1\t0\t1\t-\t00000001\t/a",
    ] {
        assert_eq!(decode(bad.as_bytes()), None, "{bad:?}");
    }
    let entries = vec![Entry {
        save_name: "a%b (2)".into(),
        base: "a%b".into(),
        index: 2,
        legacy: true,
        size: 7,
        mtime: Some(123),
        crc: 0xdead_beef,
        path: b"/r\to\nm\xff".to_vec(),
    }];
    assert_eq!(decode(&encode(&entries)), Some(entries));
}

/// Counts the bytes actually read.
struct Counting<R> {
    inner: R,
    read: u64,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

impl<R: Seek> Seek for Counting<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[test]
fn large_files_are_fingerprinted_by_their_ends() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("big.hdf");
    let size = FP_WHOLE_MAX + 5 * FP_EDGE;
    let file = File::create(&path).unwrap();
    file.set_len(size).unwrap();
    drop(file);
    let mut counting = Counting {
        inner: File::open(&path).unwrap(),
        read: 0,
    };
    let fp = fingerprint_of(&mut counting, size).unwrap();
    assert_eq!(counting.read, 2 * FP_EDGE);
    let zeros = vec![0u8; 2 * FP_EDGE as usize];
    assert_eq!(
        fp,
        Fingerprint {
            size,
            crc32: crc32fast::hash(&zeros)
        }
    );

    let small = tmp.path().join("small.bin");
    fs::write(&small, b"whole").unwrap();
    let fp = fingerprint_of(File::open(&small).unwrap(), 5).unwrap();
    assert_eq!(fp.crc32, crc32fast::hash(b"whole"));
}

#[test]
fn windows_path_keys_fold_case_and_verbatim_prefix() {
    assert_eq!(
        fold_windows_key(r"\\?\C:\Roms\Jeu.ST"),
        fold_windows_key(r"c:\roms\jeu.st")
    );
    assert_eq!(
        fold_windows_key(r"\\?\UNC\Server\Share\G.st"),
        r"\\server\share\g.st"
    );
    assert_eq!(fold_windows_key(r"D:\É\a"), r"d:\é\a");
}

#[test]
fn concurrent_resolutions_never_give_one_name_twice() {
    let fx = Fx::new();
    let roms: Vec<PathBuf> = (0..8)
        .map(|i| fx.rom(&format!("d{i}/Same.bin"), format!("rom {i}").as_bytes()))
        .collect();
    let (saves, states) = (fx.saves(), fx.states());
    let names: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = roms
            .iter()
            .map(|rom| {
                let (saves, states) = (&saves, &states);
                s.spawn(move || {
                    let spec = ContentSpec::File(rom.clone());
                    resolve_game_identity(&identity_request_for(&spec, saves, states))
                        .unwrap()
                        .save_name
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 8, "{names:?}");
    assert!(names.contains(&"Same".to_owned()));
    let Registry::Entries(entries) = read_registry(&saves.join(REGISTRY_NAME)).unwrap() else {
        panic!("registry unreadable");
    };
    assert_eq!(entries.len(), 8);
}

#[test]
fn hostile_names_stay_single_components() {
    let fx = Fx::new();
    let long = "a".repeat(250);
    for (dir, file, expected) in [
        ("x", "...bin", "game"),
        ("y", "CON.bin", "_CON"),
        (
            "z",
            &format!("{long}.bin") as &str,
            &"a".repeat(200) as &str,
        ),
    ] {
        let a = fx.rom(&format!("{dir}1/{file}"), b"1");
        let b = fx.rom(&format!("{dir}2/{file}"), b"2");
        assert_eq!(fx.resolve(&a).save_name, expected);
        let second = fx.resolve(&b);
        assert_eq!(second.save_name, format!("{expected} (2)"));
        assert_eq!(second.core_save_dir.parent(), Some(fx.saves().as_path()));
        // The longest files derived from it stay under 255 bytes.
        let tmp = format!(".{}.state9.tmp-4294967295", second.save_name);
        assert!(tmp.len() < 255);
        crate::saves::write_atomic(
            &fx.states().join(format!("{}.state9", second.save_name)),
            b"x",
            FileMode::Default,
        )
        .unwrap();
    }
}

#[test]
fn no_game_is_outside_the_registry() {
    let fx = Fx::new();
    let (saves, states) = (fx.saves(), fx.states());
    let req = identity_request_for(&ContentSpec::NoGame, &saves, &states);
    assert_eq!((req.base_name.as_str(), req.primary), (NO_GAME_NAME, None));
    let id = resolve_game_identity(&req).unwrap();
    assert_eq!(id, GameIdentity::legacy(NO_GAME_NAME, &saves));
    assert!(!saves.join(REGISTRY_NAME).exists());
}

#[test]
fn request_names_the_primary_source() {
    let (saves, states) = (Path::new("/s"), Path::new("/t"));
    let file = ContentSpec::File("/r/Sonic (World).zip".into());
    let req = identity_request_for(&file, saves, states);
    assert_eq!(req.base_name, "Sonic (World)");
    assert_eq!(req.primary, Some(Path::new("/r/Sonic (World).zip")));
    let set = |playlist: Option<&str>| ContentSpec::Media {
        set: MediaSet {
            name: "Ys: Book I".into(),
            images: vec!["/r/1.st".into(), "/r/2.st".into()],
            playlist: playlist.map(PathBuf::from),
            launch: None,
        },
        rules: crate::media::MediaRules::new(crate::media::MediaPolicy::Direct),
    };
    let with = set(Some("/r/Ys.m3u"));
    let req = identity_request_for(&with, saves, states);
    assert_eq!(req.base_name, "Ys_ Book I");
    assert_eq!(req.primary, Some(Path::new("/r/Ys.m3u")));
    let without = set(None);
    assert_eq!(
        identity_request_for(&without, saves, states).primary,
        Some(Path::new("/r/1.st"))
    );
}

#[test]
fn missing_primary_is_an_error() {
    let fx = Fx::new();
    assert!(matches!(
        fx.try_resolve(&fx.tmp.path().join("nope.bin")),
        Err(RuntimeError::Io { .. })
    ));
    assert!(!fx.saves().join(REGISTRY_NAME).exists());
}

#[test]
fn the_owner_of_a_2_0_folder_keeps_the_bare_name_after_a_new_homonym() {
    let fx = Fx::new();
    let owner = fx.rom("A/G.st", b"disk of a");
    rc2_manifest(&fx.saves().join("G.disks"), &owner, b"disk of a");
    fs::write(fx.saves().join("G.srm"), b"shared").unwrap();
    let newcomer = fx.rom("B/G.st", b"disk of b");
    let (saves, states) = (fx.saves(), fx.states());
    let resolve = |image: &Path| {
        let spec = media("G", image);
        resolve_game_identity(&identity_request_for(&spec, &saves, &states)).unwrap()
    };
    // B, never played, is launched first: `G.disks` holds A's copies, so B does not take `G`.
    let b = resolve(&newcomer);
    assert_eq!((b.save_name.as_str(), b.index), ("G (2)", 2));
    assert!(matches!(
        &b.notice,
        Some(IdentityNotice::Homonym { first, sram_copied: true, .. })
            if *first == fs::canonicalize(&owner).unwrap()
    ));
    let a = resolve(&owner);
    assert_eq!((a.save_name.as_str(), a.index), ("G", 1));
    assert_eq!(resolve(&newcomer).save_name, "G (2)");

    // A folder made by `choose_work_dir` for a replaced image (`G (3).disks`, A's old copies) is never taken by
    // a later homonym either.
    rc2_manifest(&fx.saves().join("G (3).disks"), &owner, b"disk of a");
    let third = fx.rom("C/G.st", b"disk of c");
    assert_eq!(resolve(&third).save_name, "G (4)");
    // A folder without present set copies (emptied) does not hold the name.
    let fx = Fx::new();
    let gone = fx.rom("old/H.st", b"h");
    rc2_manifest(&fx.saves().join("H.disks"), &gone, b"h");
    fs::remove_file(fx.saves().join("H.disks/Game.st")).unwrap();
    let h = fx.rom("new/H.st", b"other h");
    let (saves, states) = (fx.saves(), fx.states());
    let spec = media("H", &h);
    let id = resolve_game_identity(&identity_request_for(&spec, &saves, &states)).unwrap();
    assert_eq!(id.save_name, "H");
}

/// Moved before the first 2.1 launch, by a `.m3u` (rule 3 by content) or as a single image too large to be
/// fingerprinted whole (rule 4 gives the bare name, `choose_work_dir` finds the folder by content): the only owner
/// of `G.disks` keeps `G` and its copies.
#[test]
fn a_sole_owner_moved_before_the_first_2_1_launch_keeps_its_name_and_copies() {
    use crate::media::{MediaPolicy, prepare_media};
    let exts: Vec<String> = ["st", "m3u"].iter().map(|s| (*s).to_owned()).collect();

    // `.m3u` of two disks: rc.2 recorded the old paths of the images.
    let fx = Fx::new();
    let old1 = fx.rom("old/G (Disk 1).st", b"disk one");
    let old2 = fx.rom("old/G (Disk 2).st", b"disk two");
    let work = fx.saves().join("G.disks");
    fs::create_dir_all(&work).unwrap();
    let mut text = b"emu-rust working copies 1\n".to_vec();
    for (copy, src, data) in [
        ("G (Disk 1).st", &old1, b"disk one"),
        ("G (Disk 2).st", &old2, b"disk two"),
    ] {
        text.extend_from_slice(
            format!(
                "set\t{copy}\t\t{}\t-\t{:08x}\t",
                data.len(),
                crc32fast::hash(data)
            )
            .as_bytes(),
        );
        escape(&path_key(&fs::canonicalize(src).unwrap()), &mut text);
        text.push(b'\n');
        fs::write(work.join(copy), b"progress").unwrap();
    }
    fs::write(work.join(workcopy::MANIFEST_NAME), &text).unwrap();
    fs::rename(
        fx.tmp.path().join("roms/old"),
        fx.tmp.path().join("roms/new"),
    )
    .unwrap();
    let playlist = fx.tmp.path().join("roms/new/G.m3u");
    fs::write(&playlist, "G (Disk 1).st\nG (Disk 2).st\n").unwrap();
    let spec = ContentSpec::Media {
        set: MediaSet {
            name: "G".into(),
            images: Vec::new(),
            playlist: Some(playlist),
            launch: None,
        },
        rules: crate::media::MediaRules::new(MediaPolicy::WorkingCopy),
    };
    let (saves, states) = (fx.saves(), fx.states());
    let id = resolve_game_identity(&identity_request_for(&spec, &saves, &states)).unwrap();
    assert_eq!(
        (id.save_name.as_str(), id.index, &id.notice),
        ("G", 1, &None)
    );
    let ContentSpec::Media { mut set, .. } = spec else {
        unreachable!()
    };
    set.name = id.save_name;
    let prepared = prepare_media(
        &set,
        &crate::media::MediaRules::new(MediaPolicy::WorkingCopy),
        &exts,
        &saves,
    )
    .unwrap();
    assert_eq!(prepared.work_dir.as_deref(), Some(work.as_path()));
    assert_eq!(fs::read(work.join("G (Disk 1).st")).unwrap(), b"progress");
    // Rule 3 matched by content, not by default: the same folder as `G (2).disks` gives `G (2)`.
    fs::remove_file(saves.join(REGISTRY_NAME)).unwrap();
    fs::rename(&work, saves.join("G (2).disks")).unwrap();
    set.name = "G".into();
    let spec = ContentSpec::Media {
        set,
        rules: crate::media::MediaRules::new(MediaPolicy::WorkingCopy),
    };
    let id = resolve_game_identity(&identity_request_for(&spec, &saves, &states)).unwrap();
    assert_eq!((id.save_name.as_str(), id.index), ("G (2)", 2));

    // Single image over 64 MiB (sparse), moved.
    let fx = Fx::new();
    let size = FP_WHOLE_MAX + FP_EDGE;
    let old = fx.tmp.path().join("roms/old/G.st");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    File::create(&old).unwrap().set_len(size).unwrap();
    let mut hasher = crc32fast::Hasher::new();
    let zeros = vec![0u8; FP_EDGE as usize];
    for _ in 0..(size / FP_EDGE) {
        hasher.update(&zeros);
    }
    let crc = hasher.finalize();
    let work = fx.saves().join("G.disks");
    fs::create_dir_all(&work).unwrap();
    let copy = work.join("G.st");
    File::create(&copy).unwrap().set_len(size).unwrap();
    let mut text =
        format!("emu-rust working copies 1\nset\tG.st\t\t{size}\t-\t{crc:08x}\t").into_bytes();
    escape(&path_key(&fs::canonicalize(&old).unwrap()), &mut text);
    text.push(b'\n');
    fs::write(work.join(workcopy::MANIFEST_NAME), &text).unwrap();
    let moved = fx.tmp.path().join("roms/new/G.st");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(&old, &moved).unwrap();
    let (saves, states) = (fx.saves(), fx.states());
    let spec = media("G", &moved);
    let id = resolve_game_identity(&identity_request_for(&spec, &saves, &states)).unwrap();
    assert_eq!(
        (id.save_name.as_str(), id.index, &id.notice),
        ("G", 1, &None)
    );
    let ContentSpec::Media { set, .. } = spec else {
        unreachable!()
    };
    let prepared = prepare_media(
        &set,
        &crate::media::MediaRules::new(MediaPolicy::WorkingCopy),
        &exts,
        &saves,
    )
    .unwrap();
    assert_eq!(prepared.work_dir.as_deref(), Some(work.as_path()));
    assert_eq!(prepared.images, [copy]);
}

#[cfg(unix)]
#[test]
fn unreadable_playlist_entry_is_no_match_not_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.st");
    fs::write(&path, [1u8; 64]).unwrap();
    let mut file = SetFile::open(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::File::open(&path).is_ok() {
        return; // running as root: cannot make the file unreadable
    }
    assert_eq!(file.crc(), None);
    // The failure is remembered: no second read attempt.
    assert_eq!(file.crc(), None);
}

fn folder_spec(dir: &Path, name: &str, start: Option<&str>) -> ContentSpec {
    ContentSpec::Folder {
        dir: dir.to_path_buf(),
        name: name.into(),
        start: start.map(str::to_owned),
    }
}

#[test]
fn game_folders_are_identified_by_their_folder() {
    let fx = Fx::new();
    let (saves, states) = (fx.saves(), fx.states());
    let a = fx.tmp.path().join("roms/A/GAME");
    let b = fx.tmp.path().join("roms/B/GAME");
    fx.rom("A/GAME/GAME.EXE", b"MZ a");
    fx.rom("A/GAME/DATA/LEVEL.DAT", b"level");
    fx.rom("B/GAME/GAME.EXE", b"MZ b");

    // Named after the folder (`name` may already be a save name), primary = the folder; `start` plays no part.
    let spec = folder_spec(&b, "GAME (2)", Some("GAME.EXE"));
    let req = identity_request_for(&spec, &saves, &states);
    assert_eq!(req.base_name, "GAME");
    assert_eq!(req.primary, Some(b.as_path()));

    let resolve = |dir: &Path, start: Option<&str>| {
        let spec = folder_spec(dir, "ignored", start);
        resolve_game_identity(&identity_request_for(&spec, &saves, &states)).unwrap()
    };
    let first = resolve(&a, None);
    assert_eq!((first.save_name.as_str(), first.index), ("GAME", 1));
    let second = resolve(&b, None);
    assert_eq!((second.save_name.as_str(), second.index), ("GAME (2)", 2));
    assert_eq!(second.core_save_dir, saves.join("GAME (2)"));
    assert!(matches!(
        second.notice,
        Some(IdentityNotice::Homonym { .. })
    ));
    assert_eq!(resolve(&b, Some("GAME.EXE")).save_name, "GAME (2)");
    assert_eq!(resolve(&a, None).save_name, "GAME");
    // Fingerprint of the file list: size total + CRC32 (state headers).
    let (size, crc) = folder_fingerprint(&a, &SnapshotLimits::DEFAULT).unwrap();
    assert_eq!(first.game_fp(), Some(format!("{size}:{crc:08x}")));

    // Moved folder (same file list): saves follow. A changed folder at the same path keeps its identity.
    let moved = fx.tmp.path().join("roms/C/GAME");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(&a, &moved).unwrap();
    assert_eq!(resolve(&moved, None).save_name, "GAME");
    fx.rom("B/GAME/NEW.TXT", b"new");
    let changed = resolve(&b, None);
    assert_eq!(changed.save_name, "GAME (2)");
    assert_ne!(changed.game_fp(), second.game_fp());
}

fn resolve_scoped(fx: &Fx, spec: &ContentSpec, scope: SaveScope) -> GameIdentity {
    let (saves, states) = (fx.saves(), fx.states());
    let mut req = identity_request_for(spec, &saves, &states);
    req.scope = scope;
    resolve_game_identity(&req).unwrap()
}

/// Design v5.0 §3.2: `SaveScope::Game` is the homonym branch of 2.1, forced for every game of the core.
#[test]
fn game_scope_gives_every_game_its_folder() {
    let fx = Fx::new();
    let a = fx.rom("a/Jeu.dsk", b"one");
    let b = fx.rom("b/Jeu.dsk", b"two");
    let first = resolve_scoped(&fx, &ContentSpec::File(a.clone()), SaveScope::Game);
    assert_eq!((first.save_name.as_str(), first.index), ("Jeu", 1));
    assert_eq!(first.core_save_dir, fx.saves().join("Jeu"));
    let second = resolve_scoped(&fx, &ContentSpec::File(b), SaveScope::Game);
    assert_eq!((second.save_name.as_str(), second.index), ("Jeu (2)", 2));
    assert_eq!(second.core_save_dir, fx.saves().join("Jeu (2)"));
    // Same registry entry under either scope: only the folder handed to the core differs.
    let core = resolve_scoped(&fx, &ContentSpec::File(a), SaveScope::Core);
    assert_eq!(core.save_name, first.save_name);
    assert_eq!(core.core_save_dir, fx.saves());
    // Nothing is created by the resolution itself (the emulator creates the folder before `Core::load`).
    assert!(!fx.saves().join("Jeu").exists());

    // No content: `no-game/`, still outside the registry.
    let none = resolve_scoped(&fx, &ContentSpec::NoGame, SaveScope::Game);
    assert_eq!(none.save_name, NO_GAME_NAME);
    assert_eq!(none.core_save_dir, fx.saves().join(NO_GAME_NAME));
}

#[test]
fn game_scope_survives_an_unusable_registry() {
    let fx = Fx::new();
    fs::write(fx.saves().join(REGISTRY_NAME), b"not a registry").unwrap();
    let rom = fx.rom("Jeu.dsk", b"one");
    let id = resolve_scoped(&fx, &ContentSpec::File(rom.clone()), SaveScope::Game);
    assert_eq!(id.notice, Some(IdentityNotice::RegistryUnusable));
    assert_eq!(id.core_save_dir, fx.saves().join("Jeu"));
    // `fallback_identity` (what a caller uses when the resolution fails) follows the scope too.
    let spec = ContentSpec::File(rom);
    let (saves, states) = (fx.saves(), fx.states());
    let mut req = identity_request_for(&spec, &saves, &states);
    assert_eq!(req.fallback_identity(), GameIdentity::legacy("Jeu", &saves));
    req.scope = SaveScope::Game;
    assert_eq!(req.fallback_identity().core_save_dir, saves.join("Jeu"));
}

#[test]
#[ignore = "FR1: relocate_identities is a todo!() until then"]
fn relocate_identities_without_registry_changes_nothing() {
    let fx = Fx::new();
    let moves = [(
        fx.tmp.path().join("roms/a.sfc"),
        fx.tmp.path().join("roms/snes/a.sfc"),
    )];
    let saves = fx.tmp.path().join("saves");
    for _ in 0..2 {
        let report = relocate_identities(&saves, &moves).unwrap();
        assert_eq!(report, RelocateReport::default());
    }
}
