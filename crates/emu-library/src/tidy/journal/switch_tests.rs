//! Journal version 2 (switch with move): header, `F`, states, refusals, cut at every byte (design 8.0 §2.6,
//! ADR 0023 amendment of 2026-11-27).

use super::*;
use crate::tidy::sys::RealSys;

const UNIX: u64 = 1_700_000_000;

fn p(s: &str) -> PathBuf {
    s.split('/').collect()
}

fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Users\Jo Ë\emu rust\roms")
    } else {
        PathBuf::from("/home/jo Ë/emu rust/roms")
    }
}

fn hdr(units: u32) -> SwitchHeader {
    SwitchHeader {
        custom_id: "mysnes".into(),
        official_id: "snes".into(),
        retired_name: retired_file_name("mysnes", Retirement::Replaced, UNIX),
        units,
        keys: KeysChoice::Copy,
        prior_preference: Some(vec!["snes9x".into(), "bsnes".into()]),
        prior_profile: Some("[keys]\nup = \"W\"\n".into()),
    }
}

fn mv(n: u32, from: &str, to: &str) -> Op {
    Op::Move {
        n,
        size: 10,
        mtime: 1_700_000_000,
        from: p(from),
        to: p(to),
    }
}

/// A committed forward block of `unit` moving `mysnes/<unit>.sfc` to `snes/<unit>.sfc`.
fn forward(unit: u32) -> Vec<Op> {
    let (from, to) = (format!("mysnes/{unit}.sfc"), format!("snes/{unit}.sfc"));
    vec![
        Op::Begin(unit),
        mv(0, &from, &to),
        Op::MoveDone(0),
        Op::Commit(unit),
    ]
}

fn back(unit: u32) -> Vec<Op> {
    let (from, to) = (format!("mysnes/{unit}.sfc"), format!("snes/{unit}.sfc"));
    vec![
        Op::Begin(unit),
        mv(0, &to, &from),
        Op::MoveDone(0),
        Op::Commit(unit),
    ]
}

fn header_text(h: &SwitchHeader) -> String {
    switch_header_lines(&root(), h).unwrap().concat()
}

fn text(h: &SwitchHeader, ops: &[Op]) -> String {
    let mut t = header_text(h);
    for op in ops {
        t.push_str(&encode(op).unwrap());
    }
    t
}

fn parse_ok(t: &str) -> Journal {
    parse(t.as_bytes()).unwrap().unwrap()
}

fn refused(h: &SwitchHeader, ops: &[Op]) -> bool {
    parse(text(h, ops).as_bytes()).is_err()
}

fn flat(blocks: &[Vec<Op>]) -> Vec<Op> {
    blocks.concat()
}

/// A whole journal of two units: run, `E F I`, undo of both, `U F I`.
fn full() -> Vec<Op> {
    let mut ops = flat(&[forward(0), forward(1)]);
    ops.extend([Op::End, Op::Final, Op::Identities]);
    ops.extend(flat(&[back(1), back(0)]));
    ops.extend([Op::Undone, Op::Final, Op::Identities]);
    ops
}

#[test]
fn a_switch_journal_round_trips_through_its_six_header_lines() {
    let h = hdr(2);
    let j = parse_ok(&text(&h, &full()));
    assert_eq!(j.switch.as_ref(), Some(&h));
    assert_eq!(j.root, root());
    assert_eq!(j.state, State::UndoneMarked);
    assert!(j.forward_final && j.undo_final);
    assert_eq!(j.forward.len(), 2);
    assert_eq!(j.undo.len(), 2);
    let info = j.info(PathBuf::from("j"), 1);
    assert_eq!(
        info.switch,
        Some(SwitchMove {
            custom_id: "mysnes".into(),
            official_id: "snes".into()
        })
    );
    // A version 1 journal has none.
    let v1 = format!("{}E\n", header(&root()).unwrap());
    let j1 = parse_ok(&v1);
    assert!(j1.switch.is_none() && !j1.forward_final);
    assert_eq!(j1.info(PathBuf::from("j"), 1).switch, None);
}

#[test]
fn a_switch_journal_cut_at_every_byte_keeps_its_complete_lines() {
    let h = hdr(2);
    let ops = full();
    let full_text = text(&h, &ops);
    let header_len = header_text(&h).len();
    let mut boundaries = vec![header_len];
    let mut at = header_len;
    for op in &ops {
        at += encode(op).unwrap().len();
        boundaries.push(at);
    }
    for cut in 0..=full_text.len() {
        let parsed =
            parse(&full_text.as_bytes()[..cut]).unwrap_or_else(|e| panic!("cut at {cut}: {e}"));
        if cut < header_len {
            // Inside the header: nothing moved.
            assert_eq!(parsed, None, "cut at {cut}");
            continue;
        }
        let j = parsed.unwrap();
        let complete = boundaries.iter().filter(|&&b| b <= cut).count() - 1;
        let expected = parse_ok(&text(&h, &ops[..complete]));
        assert_eq!(
            (
                j.forward,
                j.undo,
                j.state,
                j.switch,
                j.forward_final,
                j.undo_final
            ),
            (
                expected.forward,
                expected.undo,
                expected.state,
                expected.switch,
                expected.forward_final,
                expected.undo_final
            ),
            "cut at {cut}"
        );
        assert_eq!(j.torn, !boundaries.contains(&cut), "cut at {cut}");
        assert_eq!(j.len, boundaries[complete] as u64);
    }
}

#[test]
fn a_complete_header_without_a_block_is_an_empty_running_switch() {
    let j = parse_ok(&header_text(&hdr(3)));
    assert_eq!(j.state, State::Running);
    assert!(j.forward.is_empty() && j.undo.is_empty());
    assert!(j.state.in_progress());
    assert_eq!(j.end_due(), None);
    // Without the last header line: nothing moved at all.
    let cut = header_text(&hdr(3));
    let without_q = &cut[..cut.rfind("Q ").unwrap()];
    assert_eq!(parse(without_q.as_bytes()).unwrap(), None);
}

#[test]
fn a_header_with_an_invalid_line_is_not_a_cut_header() {
    let good = header_text(&hdr(2));
    let lines: Vec<&str> = good.split_inclusive('\n').collect();
    for (index, bad) in [
        (2, "X mysnes mysnes mysnes-1700000000.replaced.toml 2\n"),
        (3, "K nothing\n"),
        (4, "P snes9x,snes9x\n"),
        (5, "Q %zz\n"),
    ] {
        // Complete lines up to the bad one: refused as soon as the bad line is complete, whatever follows.
        let upto = [&lines[..index], &[bad]].concat().concat();
        assert!(parse(upto.as_bytes()).is_err(), "{bad:?}");
        // Cut inside the bad line it is still a prefix of nothing valid only once its tag is wrong; here the
        // tag is right, so the cut line alone is accepted as a cut header.
        let cut = &upto[..upto.len() - 1];
        assert_eq!(parse(cut.as_bytes()).unwrap(), None, "{bad:?}");
    }
    // A wrong tag at the start of the line is refused at once.
    let upto = [&lines[..3], &["Z"]].concat().concat();
    assert!(parse(upto.as_bytes()).is_err());
}

#[test]
fn a_cut_header_whose_x_line_is_complete_is_checked_like_a_whole_header() {
    let good = header_text(&hdr(2));
    let lines: Vec<&str> = good.split_inclusive('\n').collect();
    let (name, other, removed) = (
        hdr(2).retired_name,
        retired_file_name("other", Retirement::Replaced, UNIX),
        retired_file_name("mysnes", Retirement::Removed, UNIX),
    );
    // A header cut after its `X` line: the retired name must be a `Replaced` one of the custom id.
    for bad in [&other, &removed] {
        let x = lines[2].replace(&name, bad);
        let cut = [&lines[..2], &[x.as_str()]].concat().concat();
        assert!(parse(cut.as_bytes()).is_err(), "{bad}");
    }
    let cut = lines[..3].concat();
    assert_eq!(parse(cut.as_bytes()).unwrap(), None);
}

#[test]
fn header_fields_outside_the_grammar_refuse_the_whole_journal() {
    let ok = hdr(2);
    let mut cases: Vec<(&str, SwitchHeader)> = Vec::new();
    for (label, change) in [
        (
            "equal ids",
            Box::new(|h: &mut SwitchHeader| h.official_id = "mysnes".into())
                as Box<dyn Fn(&mut SwitchHeader)>,
        ),
        ("upper-case id", Box::new(|h| h.custom_id = "MySnes".into())),
        ("long id", Box::new(|h| h.official_id = "a".repeat(25))),
        ("zero units", Box::new(|h| h.units = 0)),
        (
            "name of another id",
            Box::new(|h| h.retired_name = retired_file_name("other", Retirement::Replaced, UNIX)),
        ),
        (
            "name of a removal",
            Box::new(|h| h.retired_name = retired_file_name("mysnes", Retirement::Removed, UNIX)),
        ),
        (
            "path as name",
            Box::new(|h| h.retired_name = "../mysnes-1.replaced.toml".into()),
        ),
        (
            "nine cores",
            Box::new(|h| h.prior_preference = Some((0..9).map(|i| format!("c{i}")).collect())),
        ),
        (
            "duplicate core",
            Box::new(|h| h.prior_preference = Some(vec!["a".into(), "a".into()])),
        ),
        (
            "core named -",
            Box::new(|h| h.prior_preference = Some(vec!["-".into()])),
        ),
        (
            "core with a comma",
            Box::new(|h| h.prior_preference = Some(vec!["a,b".into()])),
        ),
        (
            "huge profile",
            Box::new(|h| h.prior_profile = Some("x".repeat(MAX_PRIOR_PROFILE + 1))),
        ),
    ] {
        let mut h = ok.clone();
        change(&mut h);
        cases.push((label, h));
    }
    for (label, h) in cases {
        // The writer refuses them...
        assert!(switch_header_lines(&root(), &h).is_none(), "{label}");
    }
    // ... and so does the reader, on a text written by hand.
    let good = header_text(&ok);
    for (label, bad) in [
        (
            "equal ids",
            good.replace("X mysnes snes", "X mysnes mysnes"),
        ),
        ("zero units", good.replace(" 2\n", " 0\n")),
        ("units not canonical", good.replace(" 2\n", " 02\n")),
        ("units sign", good.replace(" 2\n", " +2\n")),
        (
            "name of another id",
            good.replace("mysnes-1700000000", "other-1700000000"),
        ),
        (
            "name unix not canonical",
            good.replace("-1700000000.", "-01700000000."),
        ),
        ("name of a removal", good.replace(".replaced.toml", ".toml")),
        ("P empty", good.replace("P snes9x,bsnes", "P ")),
        ("P core -", good.replace("P snes9x,bsnes", "P snes9x,-")),
        (
            "P upper-case core",
            good.replace("P snes9x,bsnes", "P Snes9x"),
        ),
        ("K upper case", good.replace("K copy", "K Copy")),
        ("Q empty", good.replace("Q ", "Q")),
    ] {
        let t = format!("{bad}B 0\n");
        assert!(parse(t.as_bytes()).is_err(), "{label}");
    }
}

#[test]
fn preference_and_profile_are_written_canonically_and_read_back() {
    let mut h = hdr(1);
    for (preference, profile, p_line, q_line) in [
        (Some(vec![]), Some(String::new()), "P -", "Q -"),
        (None, None, "P -", "Q -"),
        (Some(vec!["a".into()]), Some("-".into()), "P a", "Q %2D"),
        (
            Some(vec!["a-b".into(), "c_d".into()]),
            Some("x -y\n%Ë".into()),
            "P a-b,c_d",
            "Q x%20-y%0A%25%C3%8B",
        ),
    ] {
        h.prior_preference = preference.clone();
        h.prior_profile = profile.clone();
        let t = header_text(&h);
        assert!(t.contains(&format!("\n{p_line}\n")), "{t}");
        assert!(t.ends_with(&format!("{q_line}\n")), "{t}");
        let back = parse_ok(&t).switch.unwrap();
        // Empty preference and empty profile read back as « none ».
        assert_eq!(back.prior_preference, preference.filter(|v| !v.is_empty()));
        assert_eq!(back.prior_profile, profile.filter(|v| !v.is_empty()));
    }
}

#[test]
fn only_the_canonical_profile_encoding_is_read() {
    let good = header_text(&hdr(1));
    let with_q = |q: &str| {
        let head = &good[..good.rfind("Q ").unwrap()];
        format!("{head}Q {q}\nE\n")
    };
    for ok in ["abc", "%20a", "%2D", "a%25b", "-a", "%C3%8B"] {
        assert!(parse(with_q(ok).as_bytes()).is_ok(), "{ok}");
    }
    for bad in [
        "%2d", "%41", "a b", "%", "%4", "%ZZ", "%2D%2D", "a\tb", "%FF", "ë",
    ] {
        // `%FF` is not UTF-8 once decoded; the others are not canonical.
        assert!(parse(with_q(bad).as_bytes()).is_err(), "{bad:?}");
    }
    // `%2D` is the profile `-`; `-` alone is « none ».
    assert_eq!(
        parse_ok(&with_q("%2D"))
            .switch
            .unwrap()
            .prior_profile
            .as_deref(),
        Some("-")
    );
    assert_eq!(parse_ok(&with_q("-")).switch.unwrap().prior_profile, None);
}

#[test]
fn f_follows_the_end_of_its_section_once_and_only_when_every_unit_is_done() {
    let h = hdr(2);
    let run = flat(&[forward(0), forward(1)]);
    let with = |tail: &[Op]| {
        let mut ops = run.clone();
        ops.extend_from_slice(tail);
        ops
    };
    assert!(!refused(&h, &with(&[Op::End, Op::Final])));
    assert!(!refused(&h, &with(&[Op::End, Op::Final, Op::Identities])));
    // No `E`, a second `F`, `F` after `I`, `F` not at once.
    assert!(refused(&h, &with(&[Op::Final])));
    assert!(refused(&h, &with(&[Op::End, Op::Final, Op::Final])));
    assert!(refused(&h, &with(&[Op::End, Op::Identities, Op::Final])));
    // A unit without its validated block: no `F` (incomplete switch), `I` allowed.
    let one = forward(0);
    assert!(refused(
        &h,
        &[one.clone(), vec![Op::End, Op::Final]].concat()
    ));
    assert!(!refused(
        &h,
        &[one.clone(), vec![Op::End, Op::Identities]].concat()
    ));
    let open = [
        one,
        vec![
            Op::Begin(1),
            mv(0, "mysnes/1.sfc", "snes/1.sfc"),
            Op::End,
            Op::Final,
        ],
    ]
    .concat();
    assert!(refused(&h, &open));
    // `I` right after the `E` of an end that is due: `F` first.
    assert!(refused(&h, &with(&[Op::End, Op::Identities])));
    // A version 1 journal knows no `F`.
    let v1 = format!("{}B 0\nC 0\nE\nF\n", header(&root()).unwrap());
    assert!(parse(v1.as_bytes()).is_err());
}

#[test]
fn the_undo_f_needs_the_forward_f_and_every_unit_back() {
    let h = hdr(2);
    let run = flat(&[forward(0), forward(1)]);
    let undo = |forward_end: &[Op], blocks: &[Vec<Op>], end: &[Op]| {
        let mut ops = run.clone();
        ops.extend_from_slice(forward_end);
        ops.extend(flat(blocks));
        ops.extend_from_slice(end);
        ops
    };
    let end_f = [Op::End, Op::Final];
    let all = [back(1), back(0)];
    assert!(!refused(&h, &undo(&end_f, &all, &[Op::Undone, Op::Final])));
    assert!(!refused(
        &h,
        &undo(&end_f, &all, &[Op::Undone, Op::Final, Op::Identities])
    ));
    // The forward section has no `F`: the undo may not either.
    assert!(refused(
        &h,
        &undo(&[Op::End], &all, &[Op::Undone, Op::Final])
    ));
    // One unit not back.
    assert!(refused(
        &h,
        &undo(&end_f, &[back(1)], &[Op::Undone, Op::Final])
    ));
    // `I` right after the `U` of an undo end that is due: `F` first; a partial undo takes `I`.
    assert!(refused(
        &h,
        &undo(&end_f, &all, &[Op::Undone, Op::Identities])
    ));
    assert!(!refused(
        &h,
        &undo(&end_f, &[back(1)], &[Op::Undone, Op::Identities])
    ));
    assert!(!refused(
        &h,
        &undo(&[Op::End], &all, &[Op::Undone, Op::Identities])
    ));
}

#[test]
fn blocks_name_units_below_n_and_header_lines_stay_in_the_header() {
    let h = hdr(2);
    // `B u` with `u >= n`, in the run and in the undo.
    assert!(refused(&h, &[Op::Begin(2)]));
    let mut ops = flat(&[forward(0), forward(1)]);
    ops.push(Op::End);
    assert!(!refused(&h, &ops));
    assert!(refused(&hdr(1), &ops));
    // X, K, P, Q lines after the header.
    for line in [
        "X mysnes snes mysnes-1700000000.replaced.toml 2",
        "K copy",
        "P -",
        "Q -",
    ] {
        let t = format!("{}{line}\n", header_text(&h));
        assert!(parse(t.as_bytes()).is_err(), "{line}");
    }
    // Header lines in another order, or the version 2 magic without them.
    let good = header_text(&h);
    let lines: Vec<&str> = good.split_inclusive('\n').collect();
    let swapped = [lines[0], lines[1], lines[3], lines[2], lines[4], lines[5]].concat();
    assert!(parse(format!("{swapped}B 0\n").as_bytes()).is_err());
    let bare = format!("{MAGIC_V2}\n{}B 0\n", lines[1]);
    assert!(parse(bare.as_bytes()).is_err());
}

#[test]
fn the_switch_end_is_due_only_when_the_last_line_says_so() {
    let h = hdr(2);
    let run = flat(&[forward(0), forward(1)]);
    let state = |tail: &[Op]| {
        let mut ops = run.clone();
        ops.extend_from_slice(tail);
        parse_ok(&text(&h, &ops))
    };
    use SwitchDirection::{Forward, Undo};
    // Forward end due after `E`; not before it, not once `F` is there.
    assert_eq!(state(&[]).end_due(), None);
    let due = state(&[Op::End]);
    assert_eq!(due.end_due(), Some(Forward));
    assert!(!due.identities_due());
    let done = state(&[Op::End, Op::Final]);
    assert_eq!(done.end_due(), None);
    assert!(done.identities_due());
    assert_eq!(done.state, State::Ended);
    assert!(!state(&[Op::End, Op::Final, Op::Identities]).identities_due());
    // Undo end due after `U` (every unit back, forward `F`).
    let mut tail = vec![Op::End, Op::Final];
    tail.extend(flat(&[back(1), back(0)]));
    tail.push(Op::Undone);
    let undone = state(&tail);
    assert_eq!(undone.end_due(), Some(Undo));
    assert!(!undone.identities_due());
    tail.push(Op::Final);
    assert_eq!(state(&tail).end_due(), None);
    assert!(state(&tail).identities_due());
    // A partial undo, or an undo of an incomplete switch, is never due.
    let mut partial = vec![Op::End, Op::Final];
    partial.extend(back(1));
    partial.push(Op::Undone);
    assert_eq!(state(&partial).end_due(), None);
    assert!(state(&partial).identities_due());
    // An incomplete switch is never due.
    let one = parse_ok(&text(&h, &[forward(0), vec![Op::End]].concat()));
    assert_eq!(one.end_due(), None);
    assert!(one.identities_due());
    // A version 1 journal never is.
    assert_eq!(
        parse_ok(&format!("{}E\n", header(&root()).unwrap())).end_due(),
        None
    );
}

#[test]
fn the_file_name_must_match_the_retired_name_and_carry_no_suffix() {
    let tmp = tempfile::tempdir().unwrap();
    let h = hdr(1);
    let body = text(&h, &[forward(0), vec![Op::End]].concat());
    let named = |name: &str| {
        let path = tmp.path().join(name);
        std::fs::write(&path, &body).unwrap();
        read(&path)
    };
    assert!(named(&file_name(UNIX as i64, 0)).unwrap().is_some());
    assert!(matches!(
        named(&file_name(UNIX as i64 + 1, 0)),
        Err(LibraryError::Journal { .. })
    ));
    assert!(matches!(
        named(&file_name(UNIX as i64, 1)),
        Err(LibraryError::Journal { .. })
    ));
}

#[test]
fn create_switch_writes_the_header_line_by_line_and_never_suffixes_the_name() {
    let tmp = tempfile::tempdir().unwrap();
    let h = hdr(1);
    let writer =
        JournalWriter::create_switch(&RealSys, tmp.path(), &root(), &h, UNIX as i64).unwrap();
    assert_eq!(writer.path(), tmp.path().join(file_name(UNIX as i64, 0)));
    assert_eq!(writer.len(), header_text(&h).len() as u64);
    assert_eq!(
        std::fs::read_to_string(writer.path()).unwrap(),
        header_text(&h)
    );
    // The name is taken: an error for the caller, who tries a later second; nothing is overwritten.
    let again = JournalWriter::create_switch(&RealSys, tmp.path(), &root(), &h, UNIX as i64);
    assert!(matches!(
        again,
        Err(LibraryError::Io { ref source, .. }) if source.kind() == std::io::ErrorKind::AlreadyExists
    ));
    // An invalid header is never written.
    let mut bad = h.clone();
    bad.units = 0;
    assert!(
        JournalWriter::create_switch(&RealSys, tmp.path(), &root(), &bad, UNIX as i64 + 1).is_err()
    );
    assert!(!tmp.path().join(file_name(UNIX as i64 + 1, 0)).exists());
}

#[test]
fn the_end_of_a_run_is_appended_after_the_header() {
    let tmp = tempfile::tempdir().unwrap();
    let h = hdr(1);
    let mut writer =
        JournalWriter::create_switch(&RealSys, tmp.path(), &root(), &h, UNIX as i64).unwrap();
    writer.append(&RealSys, &forward(0)).unwrap();
    writer.append(&RealSys, &[Op::End, Op::Final]).unwrap();
    let j = read(writer.path()).unwrap().unwrap();
    assert_eq!(j.state, State::Ended);
    assert!(j.forward_final);
}
