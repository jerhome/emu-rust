//! Launch texts and the white list of user `.m3u` lines for 8-bit computers (design v5.0 §3.2, §6).

use super::*;

struct Fixture {
    tmp: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Self {
            tmp: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(f.saves()).unwrap();
        f
    }

    fn saves(&self) -> PathBuf {
        self.tmp.path().join("saves/core")
    }

    fn file(&self, rel: &str, data: &[u8]) -> PathBuf {
        let path = self.tmp.path().join("roms").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }

    fn prepare(
        &self,
        set: &MediaSet,
        policy: MediaPolicy,
        launch: M3uLaunch,
    ) -> Result<PreparedMedia, RuntimeError> {
        let rules = MediaRules {
            m3u_launch: launch,
            ..MediaRules::new(policy)
        };
        let exts: Vec<String> = ["dsk", "d64", "m3u"].map(str::to_owned).to_vec();
        prepare_media(set, &rules, &exts, &self.saves())
    }
}

fn set(images: Vec<PathBuf>, launch: Option<&str>) -> MediaSet {
    MediaSet {
        name: "Game".into(),
        images,
        playlist: None,
        launch: launch.map(str::to_owned),
    }
}

fn text(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn command_launch_gives_a_playlist_even_for_one_image() {
    let f = Fixture::new();
    let dsk = f.file("Game.dsk", b"disk");
    let p = f
        .prepare(
            &set(vec![dsk.clone()], Some("RUN\"DISC")),
            MediaPolicy::Direct,
            M3uLaunch::Command,
        )
        .unwrap();
    let temp = p.temp_dir().unwrap().to_path_buf();
    assert_eq!(p.content, temp.join("Game.m3u"));
    assert_eq!(p.images, std::slice::from_ref(&dsk));
    assert_eq!(
        text(&p.content),
        format!("#COMMAND:RUN\"DISC\n{}\n", dsk.display())
    );
    assert_eq!(fs::read(&dsk).unwrap(), b"disk", "source untouched");
    assert!(
        fs::read_dir(f.saves()).unwrap().next().is_none(),
        "nothing in the saves"
    );

    // Without launch text (or a blank one): the image itself (4.0).
    for launch in [None, Some(""), Some("  ")] {
        let p = f
            .prepare(
                &set(vec![dsk.clone()], launch),
                MediaPolicy::Direct,
                M3uLaunch::Command,
            )
            .unwrap();
        assert_eq!(p.content, dsk, "{launch:?}");
        assert!(p.temp_dir().is_none());
    }

    // Two faces: the command first, then the faces in order.
    let b = f.file("Game (Side B).dsk", b"b");
    let p = f
        .prepare(
            &set(vec![dsk.clone(), b.clone()], Some("|RUN\"DISC")),
            MediaPolicy::Direct,
            M3uLaunch::Command,
        )
        .unwrap();
    assert_eq!(
        text(&p.content),
        format!("#COMMAND:|RUN\"DISC\n{}\n{}\n", dsk.display(), b.display())
    );
}

#[test]
fn program_launch_follows_the_first_image() {
    let f = Fixture::new();
    let a = f.file("g/Game.d64", b"a");
    let b = f.file("g/Game (Disk 2).d64", b"b");
    let p = f
        .prepare(
            &set(vec![a.clone(), b], Some("INTRO")),
            MediaPolicy::WorkingCopy,
            M3uLaunch::Program,
        )
        .unwrap();
    let work = f.saves().join("Game.disks");
    assert_eq!(p.content, work.join("Game.m3u"));
    assert_eq!(text(&p.content), "Game.d64:INTRO\nGame (Disk 2).d64\n");

    // One image: still a playlist, next to the copy.
    let f = Fixture::new();
    let p = f
        .prepare(
            &set(vec![a], Some("SECOND")),
            MediaPolicy::WorkingCopy,
            M3uLaunch::Program,
        )
        .unwrap();
    assert_eq!(p.content, f.saves().join("Game.disks/Game.m3u"));
    assert_eq!(text(&p.content), "Game.d64:SECOND\n");
}

#[test]
fn invalid_launch_texts_are_refused() {
    let f = Fixture::new();
    let dsk = f.file("Game.dsk", b"disk");
    let long = "R".repeat(65);
    let refused = [
        (M3uLaunch::Command, "RUN\"A\n#SAVEDISK:x"),
        (M3uLaunch::Command, "RUN\r"),
        (M3uLaunch::Command, long.as_str()),
        (M3uLaunch::Command, "RUN\"ÉTÉ"),
        (M3uLaunch::Command, "RUN\t"),
        (M3uLaunch::Program, "A:B"),
        (M3uLaunch::Program, "A|B"),
        (M3uLaunch::Program, "#A"),
        (M3uLaunch::None, "RUN\"DISC"),
    ];
    for (mode, launch) in refused {
        for policy in [MediaPolicy::Direct, MediaPolicy::WorkingCopy] {
            let r = f.prepare(&set(vec![dsk.clone()], Some(launch)), policy, mode);
            assert!(
                matches!(r, Err(RuntimeError::InvalidLaunch(_))),
                "{mode:?} {launch:?}: {r:?}"
            );
        }
    }
    assert!(
        !f.saves().join("Game.disks").exists(),
        "refused before any copy"
    );
    // At the bounds: 64 characters, `:` in a CPC command.
    let max = "R".repeat(64);
    for (mode, launch) in [
        (M3uLaunch::Command, max.as_str()),
        (M3uLaunch::Command, "|A:"),
    ] {
        f.prepare(
            &set(vec![dsk.clone()], Some(launch)),
            MediaPolicy::Direct,
            mode,
        )
        .unwrap();
    }
}

const USER_M3U: &[u8] = b"#EXTM3U\n# comment\n#COMMAND:RUN\"X\nGame.dsk|Face A\n#SAVEDISK:Save\n#COMMAND:RUN\"Y\n#LABEL:B\nB.dsk\n#EXTINF:1,x\n";

#[test]
fn user_playlist_keeps_a_valid_command_for_command_cores() {
    let f = Fixture::new();
    let a = f.file("Game.dsk", b"a");
    let b = f.file("B.dsk", b"b");
    let m3u = f.file("Game.m3u", USER_M3U);
    let p = f
        .prepare(
            &set(vec![m3u.clone()], None),
            MediaPolicy::Direct,
            M3uLaunch::Command,
        )
        .unwrap();
    // Regenerated in the private directory, never handed over as is.
    assert_eq!(p.content, p.temp_dir().unwrap().join("Game.m3u"));
    assert_eq!(
        text(&p.content),
        format!(
            "#COMMAND:RUN\"X\n{}|Face A\n#SAVEDISK:Save\n#LABEL:B\n{}\n",
            a.display(),
            b.display()
        )
    );
    // `# comment`, the second `#COMMAND:`, `#EXTINF:`.
    assert_eq!(p.dropped_lines, 3);
    assert_eq!(fs::read(&m3u).unwrap(), USER_M3U, "user playlist untouched");

    // The launch text of the game replaces every `#COMMAND:` (not counted as dropped).
    let p = f
        .prepare(
            &set(vec![m3u.clone()], Some("RUN\"Z")),
            MediaPolicy::Direct,
            M3uLaunch::Command,
        )
        .unwrap();
    assert_eq!(
        text(&p.content),
        format!(
            "#COMMAND:RUN\"Z\n{}|Face A\n#SAVEDISK:Save\n#LABEL:B\n{}\n",
            a.display(),
            b.display()
        )
    );
    assert_eq!(p.dropped_lines, 2);

    // An invalid `#COMMAND:` is dropped (and counted); the next valid one is kept.
    let bad = format!(
        "#COMMAND:{}\n#COMMAND:\n#COMMAND:RUN\"OK\nGame.dsk\n",
        "R".repeat(65)
    );
    let m3u = f.file("Bad.m3u", bad.as_bytes());
    let p = f
        .prepare(
            &set(vec![m3u], None),
            MediaPolicy::Direct,
            M3uLaunch::Command,
        )
        .unwrap();
    assert_eq!(
        text(&p.content),
        format!("#COMMAND:RUN\"OK\n{}\n", a.display())
    );
    assert_eq!(p.dropped_lines, 2);
}

#[test]
fn user_playlist_never_passes_a_command_to_other_cores() {
    let f = Fixture::new();
    let a = f.file("Game.dsk", b"a");
    let b = f.file("B.dsk", b"b");
    let m3u = f.file("Game.m3u", USER_M3U);

    // `Program` under `Direct` (design §10 n° 13): read back and regenerated, `#COMMAND:` dropped.
    let p = f
        .prepare(
            &set(vec![m3u.clone()], None),
            MediaPolicy::Direct,
            M3uLaunch::Program,
        )
        .unwrap();
    assert_ne!(p.content, m3u);
    assert_eq!(p.content, p.temp_dir().unwrap().join("Game.m3u"));
    assert_eq!(
        text(&p.content),
        format!(
            "{}|Face A\n#SAVEDISK:Save\n#LABEL:B\n{}\n",
            a.display(),
            b.display()
        )
    );
    assert_eq!(p.dropped_lines, 4);

    // `Program` under `WorkingCopy`, with a program: `<image>:<program>` on the first entry, before its label.
    let p = f
        .prepare(
            &set(vec![m3u.clone()], Some("INTRO")),
            MediaPolicy::WorkingCopy,
            M3uLaunch::Program,
        )
        .unwrap();
    assert_eq!(
        text(&p.content),
        "Game.dsk:INTRO|Face A\n#SAVEDISK:Save\n#LABEL:B\nB.dsk\n"
    );
    assert_eq!(p.dropped_lines, 4);

    // `None` under `WorkingCopy`: dropped too.
    let f2 = Fixture::new();
    f2.file("Game.dsk", b"a");
    f2.file("B.dsk", b"b");
    let m3u2 = f2.file("Game.m3u", USER_M3U);
    let p = f2
        .prepare(
            &set(vec![m3u2.clone()], None),
            MediaPolicy::WorkingCopy,
            M3uLaunch::None,
        )
        .unwrap();
    assert!(!text(&p.content).contains("#COMMAND"));
    assert_eq!(p.dropped_lines, 4);

    // `None` under `Direct`: handed over untouched (2.0), nothing counted.
    let p = f2
        .prepare(
            &set(vec![m3u2.clone()], None),
            MediaPolicy::Direct,
            M3uLaunch::None,
        )
        .unwrap();
    assert_eq!((p.content, p.dropped_lines), (m3u2, 0));
}

#[test]
fn program_refuses_an_image_path_containing_a_colon() {
    let f = Fixture::new();
    let a = f.file("C:dir/A.dsk", b"a");
    let b = f.file("C:dir/B.dsk", b"b");
    let err = f
        .prepare(
            &set(vec![a, b], Some("INTRO")),
            MediaPolicy::Direct,
            M3uLaunch::Program,
        )
        .unwrap_err();
    assert!(matches!(err, RuntimeError::InvalidLaunch(_)), "{err:?}");
}
