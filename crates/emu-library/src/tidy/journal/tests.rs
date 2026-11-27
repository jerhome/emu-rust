use super::*;
use crate::tidy::sys::RealSys;

fn p(s: &str) -> PathBuf {
    s.split('/').collect()
}

fn mv(n: u32, size: u64, from: &str, to: &str) -> Op {
    Op::Move {
        n,
        size,
        mtime: 1_700_000_000 + i64::from(n),
        from: p(from),
        to: p(to),
    }
}

fn whole(from: &str, to: &str) -> Op {
    Op::Whole {
        files: 3,
        size: 300,
        mtime: 1_700_000_100,
        from: p(from),
        to: p(to),
    }
}

/// A journal using every record: two committed units, an abandoned one, the undo of unit 7, identities.
fn all_ops() -> Vec<Op> {
    vec![
        Op::Begin(0),
        Op::DirCreated(p("psx")),
        Op::Part(p("psx/Game (Europe).emu-rust-part")),
        Op::DirCreated(p("psx/Game (Europe)/sub")),
        mv(
            0,
            10,
            "Game (Europe).cue",
            "psx/Game (Europe)/Game (Europe).cue",
        ),
        mv(
            1,
            2352,
            "Game (Europe) (Track 1).bin",
            "psx/Game (Europe)/sub/Game (Europe) (Track 1).bin",
        ),
        Op::MoveDone(0),
        Op::MoveDone(1),
        Op::Commit(0),
        Op::Begin(7),
        whole("vrac/Doom", "dos/Doom"),
        Op::WholeDone,
        Op::DirRemoved(p("vrac")),
        Op::Commit(7),
        Op::Begin(8),
        mv(0, 5, "x.sfc", "snes/x.sfc"),
        Op::End,
        Op::Identities,
        Op::Begin(7),
        Op::DirCreated(p("vrac")),
        Op::Whole {
            files: 3,
            size: 300,
            mtime: 1_700_000_101,
            from: p("dos/Doom"),
            to: p("vrac/Doom"),
        },
        Op::WholeDone,
        Op::DirRemoved(p("dos")),
        Op::Commit(7),
        Op::Undone,
        Op::Identities,
    ]
}

fn text(root: &Path, ops: &[Op]) -> String {
    let mut t = header(root).unwrap();
    for op in ops {
        t.push_str(&encode(op).unwrap());
    }
    t
}

fn abs_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Users\Jo Ë\emu rust\roms")
    } else {
        PathBuf::from("/home/jo Ë/emu rust/roms")
    }
}

fn parse_ok(t: &str) -> Journal {
    parse(t.as_bytes()).unwrap().unwrap()
}

fn refused(ops: &[Op]) -> bool {
    parse(text(&abs_root(), ops).as_bytes()).is_err()
}

#[test]
fn every_record_round_trips_and_builds_the_sections() {
    let root = abs_root();
    let j = parse_ok(&text(&root, &all_ops()));
    assert_eq!(j.root, root);
    assert_eq!(j.state, State::UndoneMarked);
    assert!(!j.torn);
    assert_eq!(
        j.forward
            .iter()
            .map(|b| (b.unit, b.committed))
            .collect::<Vec<_>>(),
        vec![(0, true), (7, true), (8, false)]
    );
    assert_eq!(j.undo.len(), 1);
    assert_eq!(j.forward[0].final_dir(), Some(p("psx/Game (Europe)")));
    assert_eq!(j.committed_moves(), 3);
    // Unit 7 came back: its pair is swapped; unit 0 stayed tidied; the abandoned unit 8 is not listed.
    assert_eq!(
        j.net_moves(),
        vec![
            (
                p("Game (Europe).cue"),
                p("psx/Game (Europe)/Game (Europe).cue")
            ),
            (
                p("Game (Europe) (Track 1).bin"),
                p("psx/Game (Europe)/sub/Game (Europe) (Track 1).bin")
            ),
            (p("dos/Doom"), p("vrac/Doom")),
        ]
    );
    assert_eq!(j.len, text(&root, &all_ops()).len() as u64);
}

#[test]
fn escaping_keeps_fields_free_of_spaces_and_separators() {
    let line = encode(&Op::Move {
        n: 3,
        size: 0,
        mtime: 0,
        from: p("a b%c/é\u{7}.st"),
        to: p("atarist/a b%c/é\u{7}.st"),
    })
    .unwrap();
    assert_eq!(
        line,
        "M 3 0 0 a%20b%25c/%C3%A9%07.st atarist/a%20b%25c/%C3%A9%07.st\n"
    );
    assert_eq!(line.matches(' ').count(), 5);
    let header = header(&abs_root()).unwrap();
    assert!(header.starts_with("emu-rust tidy 1\nR "));
    assert!(
        !header["emu-rust tidy 1\nR ".len()..]
            .trim_end()
            .contains(' ')
    );
}

#[test]
fn unsafe_paths_and_bad_numbers_are_never_written_nor_read() {
    for bad in ["../x", "/abs", "a/../b", "./a", ""] {
        assert_eq!(encode(&Op::Part(PathBuf::from(bad))), None, "{bad}");
    }
    assert_eq!(header(Path::new("relative/roms")), None);
    let base = format!("{}B 0\n", header(&abs_root()).unwrap());
    for bad in [
        "S ..",
        "S a/../b",
        "S %2E%2E",
        "S a/%2F/b",
        "S a//b",
        "S a%00b",
        "S a%zz",
        "S a%2f",
        "S a b",
        "M 1 a",
        "M 0 1 2 a",
        "M x 1 2 a b",
        "M 0 +1 2 a b",
        "M 0 01 2 a b",
        "M 0 1 -2 a b",
        "M 0 1 9223372036854775808 a b",
        "M 0 18446744073709551616 2 a b",
        "M 00 1 2 a b",
        "m -1",
        "B 99999999999",
        "B 01",
        "X",
        "E extra",
        "",
    ] {
        let t = format!("{base}{bad}\n");
        assert!(parse(t.as_bytes()).is_err(), "{bad:?}");
    }
    let t = format!("{base}M 0 18446744073709551615 9223372036854775807 a b\n");
    assert!(parse(t.as_bytes()).is_ok(), "largest values are accepted");
    // Version 2 exists since 8.0 (see `switch_tests`); another one is still unknown.
    assert!(parse(b"emu-rust tidy 3\nR /x\n").is_err());
    assert!(parse(b"emu-rust tidy 1\nR relative\n").is_err());
    assert!(parse(b"emu-rust tidy 1\nR /x\n\xff\n").is_err());
    assert!(parse(b"garbage").is_err());
}

#[test]
fn an_empty_file_or_a_cut_header_means_nothing_moved() {
    let full = header(&abs_root()).unwrap();
    for cut in 0..full.len() {
        assert_eq!(parse(&full.as_bytes()[..cut]), Ok(None), "cut at {cut}");
    }
    assert!(parse(full.as_bytes()).unwrap().is_some());
    assert!(parse(b"emu-rust tidy 1\nX").is_err());
    assert!(parse(b"emu-rust tidx").is_err());
}

#[test]
fn error_messages_never_quote_the_content() {
    let t = format!("{}B secret-name\n", header(&abs_root()).unwrap());
    let message = parse(t.as_bytes()).unwrap_err();
    assert_eq!(message, "ligne 3 invalide");
}

/// FV0 2 and design §3.4.1 « Refus du journal entier », cases of every rule.
#[test]
fn the_grammar_refuses_the_whole_journal() {
    let b0 = Op::Begin(0);
    let c0 = Op::Commit(0);
    let m0 = mv(0, 1, "a.sfc", "snes/a.sfc");
    let part = |s: &str| Op::Part(p(s));
    let cases: Vec<(&str, Vec<Op>)> = vec![
        // 1
        ("M outside a block", vec![m0.clone()]),
        ("D+ outside a block", vec![Op::DirCreated(p("snes"))]),
        ("S outside a block", vec![part("psx/G.emu-rust-part")]),
        ("W outside a block", vec![whole("a", "dos/a")]),
        ("m outside a block", vec![Op::MoveDone(0)]),
        (
            "body after a commit",
            vec![b0.clone(), c0.clone(), Op::DirCreated(p("x"))],
        ),
        ("C of another unit", vec![b0.clone(), Op::Commit(1)]),
        ("B seen twice", vec![b0.clone(), c0.clone(), b0.clone()]),
        // 2
        ("n twice", vec![b0.clone(), m0.clone(), m0.clone()]),
        ("m without M", vec![b0.clone(), Op::MoveDone(0)]),
        (
            "m twice",
            vec![b0.clone(), m0.clone(), Op::MoveDone(0), Op::MoveDone(0)],
        ),
        ("w without W", vec![b0.clone(), Op::WholeDone]),
        (
            "w twice",
            vec![
                b0.clone(),
                whole("a", "dos/a"),
                Op::WholeDone,
                Op::WholeDone,
            ],
        ),
        // 3
        (
            "two S",
            vec![
                b0.clone(),
                part("psx/G.emu-rust-part"),
                part("psx/H.emu-rust-part"),
            ],
        ),
        (
            "S after M",
            vec![
                b0.clone(),
                mv(0, 1, "a.cue", "psx/G/a.cue"),
                part("psx/G.emu-rust-part"),
            ],
        ),
        (
            "W with M",
            vec![b0.clone(), m0.clone(), whole("a", "dos/a")],
        ),
        (
            "two W",
            vec![b0.clone(), whole("a", "dos/a"), whole("b", "dos/b")],
        ),
        (
            "W with S",
            vec![b0.clone(), part("dos/a.emu-rust-part"), whole("a", "dos/a")],
        ),
        ("S badly named", vec![b0.clone(), part("psx/G")]),
        (
            "S only the suffix",
            vec![b0.clone(), part("psx/.emu-rust-part")],
        ),
        (
            "M outside <D>",
            vec![
                b0.clone(),
                part("psx/G.emu-rust-part"),
                mv(0, 1, "a.cue", "psx/H/a.cue"),
            ],
        ),
        (
            "M onto <D> itself",
            vec![
                b0.clone(),
                part("psx/G.emu-rust-part"),
                mv(0, 1, "a.cue", "psx/G"),
            ],
        ),
        (
            "D+ neither under <D> nor its ancestor",
            vec![
                b0.clone(),
                part("psx/G.emu-rust-part"),
                Op::DirCreated(p("snes")),
            ],
        ),
        (
            "earlier D+ unrelated to <D>",
            vec![
                b0.clone(),
                Op::DirCreated(p("snes")),
                part("psx/G.emu-rust-part"),
            ],
        ),
        // 4
        (
            "C with a link missing",
            vec![b0.clone(), m0.clone(), c0.clone()],
        ),
        (
            "C with a rename missing",
            vec![b0.clone(), whole("a", "dos/a"), c0.clone()],
        ),
        // 6
        ("second E", vec![Op::End, Op::End]),
        ("I without closing", vec![Op::Identities]),
        ("two I", vec![Op::End, Op::Identities, Op::Identities]),
        (
            "anything after U I",
            vec![Op::End, Op::Undone, Op::Identities, Op::End],
        ),
        ("B after U", vec![Op::End, Op::Undone, b0.clone()]),
        ("U before E", vec![Op::Undone]),
        (
            "E in the undo section",
            vec![
                b0.clone(),
                m0.clone(),
                Op::MoveDone(0),
                c0.clone(),
                Op::End,
                b0.clone(),
                Op::End,
            ],
        ),
        // 7 (RV1 I1)
        (
            "M changing only the case",
            vec![b0.clone(), mv(0, 1, "Snes/a.sfc", "snes/A.SFC")],
        ),
        (
            "M changing only the Unicode form",
            vec![b0.clone(), mv(0, 1, "jeu/e\u{301}.sfc", "jeu/\u{e9}.sfc")],
        ),
        (
            "W changing only the case",
            vec![b0.clone(), whole("DOS/Doom", "dos/doom")],
        ),
    ];
    for (what, ops) in cases {
        assert!(refused(&ops), "{what}");
    }
    // Allowed: `D+` of the ancestors of `<D>` before `S`, and below `<D>` after it (RV1 I4).
    assert!(!refused(&[
        b0.clone(),
        Op::DirCreated(p("psx")),
        part("psx/G.emu-rust-part"),
        Op::DirCreated(p("psx/G/sub")),
        mv(0, 1, "a.cue", "psx/G/sub/a.cue"),
    ]));
}

#[test]
fn the_undo_section_must_invert_a_committed_unit() {
    let forward = vec![
        Op::Begin(0),
        mv(0, 1, "a.sfc", "snes/a.sfc"),
        mv(1, 2, "a.srm", "snes/a.srm"),
        Op::MoveDone(0),
        Op::MoveDone(1),
        Op::Commit(0),
        Op::Begin(1),
        mv(0, 3, "b.sfc", "snes/b.sfc"),
        Op::End,
    ];
    let with = |undo: &[Op]| {
        let mut ops = forward.clone();
        ops.extend_from_slice(undo);
        ops
    };
    let back0 = mv(0, 1, "snes/a.sfc", "a.sfc");
    let back1 = mv(1, 2, "snes/a.srm", "a.srm");
    let good = with(&[
        Op::Begin(0),
        back0.clone(),
        back1.clone(),
        Op::MoveDone(0),
        Op::MoveDone(1),
        Op::Commit(0),
        Op::Undone,
    ]);
    assert!(!refused(&good));
    // The undo mtime is the one found at the destination: it may differ.
    let other_mtime = Op::Move {
        n: 0,
        size: 1,
        mtime: 5,
        from: p("snes/a.sfc"),
        to: p("a.sfc"),
    };
    assert!(!refused(&with(&[Op::Begin(0), other_mtime])));
    for (what, undo) in [
        (
            "S in the undo",
            vec![Op::Begin(0), Op::Part(p("x.emu-rust-part"))],
        ),
        ("uncommitted forward unit", vec![Op::Begin(1)]),
        ("unknown unit", vec![Op::Begin(9)]),
        (
            "not swapped",
            vec![Op::Begin(0), mv(0, 1, "a.sfc", "snes/a.sfc")],
        ),
        (
            "other size",
            vec![Op::Begin(0), mv(0, 9, "snes/a.sfc", "a.sfc")],
        ),
        (
            "other n",
            vec![Op::Begin(0), mv(1, 1, "snes/a.sfc", "a.sfc")],
        ),
        (
            "a forward move without inverse in a committed block",
            vec![Op::Begin(0), back0.clone(), Op::MoveDone(0), Op::Commit(0)],
        ),
        ("unit twice", vec![Op::Begin(0), Op::Begin(0)]),
    ] {
        assert!(refused(&with(&undo)), "{what}");
    }
}

#[test]
fn states_and_info_follow_the_last_lines() {
    let root = abs_root();
    let base = vec![
        Op::Begin(0),
        mv(0, 1, "a.sfc", "snes/a.sfc"),
        Op::MoveDone(0),
        Op::Commit(0),
    ];
    let state = |extra: &[Op]| {
        let mut ops = base.clone();
        ops.extend_from_slice(extra);
        parse_ok(&text(&root, &ops))
    };
    let undo0 = [Op::Begin(0), mv(0, 1, "snes/a.sfc", "a.sfc")];
    assert_eq!(state(&[]).state, State::Running);
    assert_eq!(state(&[Op::End]).state, State::Ended);
    assert_eq!(state(&[Op::End, Op::Identities]).state, State::EndedMarked);
    assert_eq!(state(&[Op::End, undo0[0].clone()]).state, State::Undoing);
    // The undo section opened after an `I`: that `I` no longer counts (state 4, recognised by the `E` upstream).
    let undoing = state(&[Op::End, Op::Identities, undo0[0].clone(), undo0[1].clone()]);
    assert_eq!(undoing.state, State::Undoing);
    let info = undoing.info(PathBuf::from("j"), 1);
    assert!(!info.complete && !info.undone && !info.identities_done);
    assert_eq!(state(&[Op::End, Op::Undone]).state, State::Undone);
    let done = state(&[Op::End, Op::Undone, Op::Identities]);
    let info = done.info(PathBuf::from("j"), 1);
    assert!(info.complete && info.undone && info.identities_done && !info.can_undo());
    let info = state(&[Op::End]).info(PathBuf::from("j"), 1);
    assert!(info.complete && !info.undone && !info.identities_done && info.can_undo());
    assert_eq!(info.moved, 1);
}

/// A crash may cut the journal after any byte (FV0 15): every prefix is either « nothing moved », or a valid
/// journal holding exactly the complete lines; a prefix cut inside a line drops that line (and only it).
#[test]
fn a_journal_cut_at_every_byte_keeps_its_complete_lines() {
    let root = abs_root();
    let ops = all_ops();
    let full = text(&root, &ops);
    let header_len = header(&root).unwrap().len();
    let mut boundaries = vec![header_len];
    let mut at = header_len;
    for op in &ops {
        at += encode(op).unwrap().len();
        boundaries.push(at);
    }
    for cut in 0..=full.len() {
        let parsed = parse(&full.as_bytes()[..cut]).unwrap_or_else(|e| panic!("cut at {cut}: {e}"));
        if cut < header_len {
            assert_eq!(parsed, None, "cut at {cut}");
            continue;
        }
        let j = parsed.unwrap();
        let complete = boundaries.iter().filter(|&&b| b <= cut).count() - 1;
        let expected = parse_ok(&text(&root, &ops[..complete]));
        assert_eq!(
            (j.forward, j.undo, j.state),
            (expected.forward, expected.undo, expected.state),
            "cut at {cut}"
        );
        assert_eq!(j.torn, !boundaries.contains(&cut), "cut at {cut}");
        assert_eq!(j.len, boundaries[complete] as u64);
    }
}

#[test]
fn names_are_strict_and_listing_is_ordered() {
    assert_eq!(
        parse_name("journal-1700000000.txt"),
        Some((1_700_000_000, 0))
    );
    assert_eq!(
        parse_name("journal-1700000000-2.txt"),
        Some((1_700_000_000, 2))
    );
    assert_eq!(parse_name("journal-0.txt"), Some((0, 0)));
    for bad in [
        "journal-.txt",
        "journal-12a.txt",
        "journal-1.txt.bak",
        "lock",
        "journal--1.txt",
        "journal-+1.txt",
        "journal-01.txt",
        "journal-1-0.txt",
        "journal-1-01.txt",
        "journal-1-+2.txt",
    ] {
        assert_eq!(parse_name(bad), None, "{bad}");
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(list(&dir.path().join("absent")).unwrap().is_empty());
    let root = dir.path().canonicalize().unwrap();
    let a = JournalWriter::create(&RealSys, dir.path(), &root, 20).unwrap();
    let b = JournalWriter::create(&RealSys, dir.path(), &root, 20).unwrap();
    let c = JournalWriter::create(&RealSys, dir.path(), &root, 3).unwrap();
    fs::write(dir.path().join("lock"), b"").unwrap();
    let listed: Vec<PathBuf> = list(dir.path())
        .unwrap()
        .into_iter()
        .map(|l| l.path)
        .collect();
    assert_eq!(
        listed,
        vec![
            c.path().to_path_buf(),
            a.path().to_path_buf(),
            b.path().to_path_buf()
        ]
    );
    assert!(b.path().ends_with("journal-20-1.txt"));
}

#[test]
fn writer_appends_synced_lines() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut w = JournalWriter::create(&RealSys, dir.path(), &root, 42).unwrap();
    w.append(&RealSys, &all_ops()[..17]).unwrap();
    assert!(
        w.append(&RealSys, &[Op::Part(PathBuf::from("../escape"))])
            .is_err()
    );
    let path = w.path().to_path_buf();
    assert_eq!(w.len(), fs::metadata(&path).unwrap().len());
    drop(w);
    let j = read(&path).unwrap().unwrap();
    assert_eq!(j.root, root);
    assert_eq!(j.state, State::Ended);
    let mut w = JournalWriter::append_to(&RealSys, &path).unwrap();
    w.append(&RealSys, &[Op::Identities]).unwrap();
    assert_eq!(read(&path).unwrap().unwrap().state, State::EndedMarked);
}

/// I1: a torn last line is cut away before appending, never glued to the next record, never « repaired ».
#[test]
fn appending_cuts_a_torn_line_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = dir.path().join("journal-1.txt");
    let ops = [
        Op::Begin(0),
        mv(0, 1, "a.sfc", "snes/a.sfc"),
        Op::MoveDone(0),
    ];
    let good = text(&root, &ops);
    for torn in ["C", "C 0", "M 1 5 5 b.sfc snes/b"] {
        fs::write(&path, format!("{good}{torn}")).unwrap();
        let j = read(&path).unwrap().unwrap();
        assert!(j.torn);
        let mut w = JournalWriter::append_to(&RealSys, &path).unwrap();
        assert_eq!(w.len(), good.len() as u64);
        w.append(&RealSys, &[Op::End]).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{good}E\n"),
            "{torn:?}"
        );
        let j = read(&path).unwrap().unwrap();
        assert!(!j.torn);
        assert_eq!(j.state, State::Ended);
        assert!(!j.forward[0].committed);
    }
}

/// FV1a I4 d: cutting a torn line goes through `Sys` (a fault there is an error, nothing appended).
#[test]
fn cutting_a_torn_line_goes_through_sys() {
    use crate::tidy::sys::faulty::{Faulty, Mode};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = dir.path().join("journal-1.txt");
    let good = text(&root, &[Op::Begin(0)]);
    fs::write(&path, format!("{good}C 0")).unwrap();
    for mode in [Mode::Once, Mode::Crash, Mode::DoneButFailed] {
        assert!(JournalWriter::append_to(&Faulty::new(0, mode), &path).is_err());
    }
    // The done-but-failed cut is on disk; the next opening finds nothing to cut and makes no call.
    assert_eq!(fs::read_to_string(&path).unwrap(), good);
    let counter = Faulty::new(u64::MAX, Mode::Once);
    JournalWriter::append_to(&counter, &path).unwrap();
    assert_eq!(counter.calls(), 0);
}

#[test]
fn an_oversized_journal_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal-1.txt");
    let mut t = header(&abs_root()).unwrap();
    t.push_str(&"E\n".repeat((MAX_JOURNAL_SIZE / 2) as usize));
    fs::write(&path, t).unwrap();
    assert!(matches!(read(&path), Err(LibraryError::Journal { .. })));
}

#[test]
fn windows_names_are_checked() {
    for bad in [
        "a.", "a ", "CON", "con.txt", "Lpt1", "COM9.bin", "a:b", "a\\b", "a?", "NUL .x",
    ] {
        assert!(bad_windows_component(bad.as_bytes()), "{bad}");
    }
    for good in ["a", "COM0", "COM10", "console", "a.b", ".hidden", "LPT"] {
        assert!(!bad_windows_component(good.as_bytes()), "{good}");
    }
}

#[test]
fn folding_matches_case_and_unicode_forms() {
    assert!(same_place(&p("PSX/Game.CUE"), &p("psx/game.cue")));
    assert!(same_place(&p("e\u{301}t\u{e9}"), &p("\u{c9}T\u{c9}")));
    assert!(!same_place(&p("psx/a.cue"), &p("a.cue")));
    assert_eq!(final_of_part(&p("psx/G.emu-rust-part")), Some(p("psx/G")));
    assert_eq!(final_of_part(&p("psx/.emu-rust-part")), None);
    assert_eq!(final_of_part(&p("psx/G")), None);
}
