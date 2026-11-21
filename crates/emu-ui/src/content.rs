//! From the files of a game to the `ContentSpec` the runtime loads (ADR 0015 §5). Pure, tested.

use std::path::{Path, PathBuf};

use emu_assets::cores::{CoreId, CoreMediaPolicy, CorePreferences, M3uLaunchPref, SaveScopePref};
use emu_runtime::{
    ContentSpec, GameIdentity, M3uLaunch, MediaPolicy, MediaRules, MediaSet, SaveScope,
};

/// What to boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameContent {
    /// One file (1.0), the disks of one game in order, or one `.m3u`.
    Files(Vec<PathBuf>),
    /// No content (`SET_SUPPORT_NO_GAME`): the computer's desktop.
    NoGame,
    /// Unpacked game folder (DOS, design v4.0 §4), checked by `computer::dos::spec_for_content` (folder policy of
    /// the core, `start` at the root). `start`: program chosen for this launch (`[dos] start` or « Lancer un autre
    /// programme… »).
    Folder { dir: PathBuf, start: Option<String> },
}

impl GameContent {
    /// First file, if any (pad style detection, logs).
    pub fn first(&self) -> Option<&Path> {
        match self {
            Self::Files(f) => f.first().map(PathBuf::as_path),
            Self::NoGame => None,
            Self::Folder { dir, .. } => Some(dir),
        }
    }
}

/// `.m3u` or `.m3u8` (same format, UTF-8: design v4.0 §10 n° 14).
fn is_m3u(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Floppy image extensions of the computer systems (`m3u` excluded: playlists are handled apart).
/// True when `path` has an extension of a console of the catalog (`systems.toml`, kind Console) and of no
/// computer: only then is a file known to be a read-only cartridge.
fn is_console_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(emu_library::systems_for_extension)
        .is_some_and(|s| {
            !s.is_empty() && s.iter().all(|i| i.kind == emu_library::SystemKind::Console)
        })
}

fn has_zip_ext(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// §2.4 (design v2.1-identite-disques): a `.zip` whose retained entries (non-empty files) all have a console
/// extension is a console ROM archive. Only the central directory is read; an unreadable or empty zip is not.
fn is_console_zip(path: &Path) -> bool {
    has_zip_ext(path)
        && emu_runtime::archive_entry_names(path).is_ok_and(|names| {
            !names.is_empty() && names.iter().all(|n| is_console_file(Path::new(n)))
        })
}

/// `direct` computer cores that declare `zip` but insert no floppy from it (« Drive A: disc missing », checked
/// with the real core): a zip is handed to them as a media set so the runtime extracts the images. No
/// `cores.toml` key says this yet, hence the local list. hatarib is left out on purpose: it reads zips itself
/// (Hatari), not checked with the real core, and the name of the modified floppy it writes to saves/ would change.
/// Working-copy cores (hatari, puae, vice) always get media sets; dosbox_pure reads zips (`media_direct`).
const NO_FLOPPY_FROM_ZIP: &[&str] = &["cap32"];

fn is_known(prefs: &CorePreferences, id: &CoreId) -> bool {
    prefs
        .systems()
        .any(|(_, cores)| cores.iter().any(|c| c == id))
}

/// Name of a disk set: the library group name (without `(Disk N of M)`), else the first file's stem.
pub fn media_set_name(files: &[PathBuf]) -> String {
    let (groups, _) = emu_library::group_media(files);
    groups
        .into_iter()
        .find(|g| g.members.first() == files.first())
        .map(|g| g.name)
        .or_else(|| files.first().map(|f| stem(f)))
        .unwrap_or_default()
}

/// Media policy rule: `working_copy` cores always get working copies; a core absent from the preferences gets
/// one as soon as a file is a floppy image (prudent default); everything else is `Direct`.
/// `File` for a single file of a `direct` core (1.0 behaviour, console zips included); `Media` for several disks, a
/// `.m3u`, any content of a `working_copy` core (hatari writes into its disks: the user's file must never be
/// handed over, even alone), or a `.zip` for a core of [`NO_FLOPPY_FROM_ZIP`] (cap32: the runtime extracts the
/// images instead); `NoGame` without files.
pub fn content_spec(core_id: &str, content: &GameContent, prefs: &CorePreferences) -> ContentSpec {
    let files = match content {
        GameContent::NoGame => return ContentSpec::NoGame,
        GameContent::Files(f) if f.is_empty() => return ContentSpec::NoGame,
        GameContent::Files(f) => f,
        // The runtime names the snapshot after the identity's `save_name`; `name` is the folder's own name.
        GameContent::Folder { dir, start } => {
            return ContentSpec::Folder {
                dir: dir.clone(),
                name: dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                start: start.clone(),
            };
        }
    };
    let id = CoreId::new(core_id);
    let policy = match id.as_ref().map(|id| prefs.media_policy(id)) {
        Some(CoreMediaPolicy::WorkingCopy) => MediaPolicy::WorkingCopy,
        // Unknown core (renamed file, `--core /x/hatari-old.so`): it may write into its disks, so anything
        // that is not surely a console file (floppies, hard disks, `.m3u`, unknown extensions) gets a working
        // copy. A zip of console ROMs only is a cartridge too (§2.4). Cores known to the preferences keep their
        // explicit policy.
        _ if id.as_ref().is_none_or(|id| !is_known(prefs, id))
            && !files
                .iter()
                .all(|f| is_console_file(f) || is_console_zip(f)) =>
        {
            MediaPolicy::WorkingCopy
        }
        _ => MediaPolicy::Direct,
    };
    let mut rules = MediaRules::new(policy);
    if let Some(id) = &id {
        rules.media_direct = prefs.media_direct(id);
        rules.m3u_launch = m3u_launch(prefs, id);
    }
    if files.len() == 1 && is_m3u(&files[0]) {
        return ContentSpec::Media {
            set: MediaSet {
                name: stem(&files[0]),
                images: Vec::new(),
                playlist: Some(files[0].clone()),
                launch: None,
            },
            rules,
        };
    }
    let floppy_zip =
        files.len() == 1 && has_zip_ext(&files[0]) && NO_FLOPPY_FROM_ZIP.contains(&core_id);
    if files.len() == 1 && policy == MediaPolicy::Direct && !floppy_zip {
        return ContentSpec::File(files[0].clone());
    }
    ContentSpec::Media {
        set: MediaSet {
            name: media_set_name(files),
            images: files.clone(),
            playlist: None,
            launch: None,
        },
        rules,
    }
}

/// `cores.toml` `m3u_launch` of a core as the runtime takes it (`None` without the key).
pub(crate) fn m3u_launch(prefs: &CorePreferences, id: &CoreId) -> M3uLaunch {
    match prefs.m3u_launch(id) {
        Some(M3uLaunchPref::Command) => M3uLaunch::Command,
        Some(M3uLaunchPref::Program) => M3uLaunch::Program,
        None => M3uLaunch::None,
    }
}

/// `cores.toml` `save_scope` of a core (`Core` for an unknown core).
pub(crate) fn save_scope(prefs: &CorePreferences, core_id: &str) -> SaveScope {
    match CoreId::new(core_id).map(|id| prefs.save_scope(&id)) {
        Some(SaveScopePref::Game) => SaveScope::Game,
        _ => SaveScope::Core,
    }
}

/// Puts the launch text of a game (`[media] launch`, or `--launch`) into its content (design v5.0 §2.1-§2.2). The
/// text must have passed `computer::eightbit::validate_launch` for `kind`. A single file of a `direct` core becomes
/// a one-image set (the runtime then generates the `.m3u` carrying the command). VICE matches the program name
/// against the lower-case PETSCII directory as read from the host, so a `Program` text is lower-cased (an
/// upper-case `INTRO` gives `?FILE NOT FOUND` on the real core). Blank text, no content or a game folder:
/// unchanged.
pub(crate) fn with_launch(
    spec: ContentSpec,
    text: Option<&str>,
    kind: M3uLaunch,
    rules_for_file: impl FnOnce() -> MediaRules,
) -> ContentSpec {
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return spec;
    };
    let text = if kind == M3uLaunch::Program {
        text.to_ascii_lowercase()
    } else {
        text.to_owned()
    };
    match spec {
        ContentSpec::Media { mut set, rules } => {
            set.launch = Some(text);
            ContentSpec::Media { set, rules }
        }
        ContentSpec::File(file) => {
            let files = vec![file];
            ContentSpec::Media {
                set: MediaSet {
                    name: media_set_name(&files),
                    images: files,
                    playlist: None,
                    launch: Some(text),
                },
                rules: rules_for_file(),
            }
        }
        other => other,
    }
}

/// Rules of a single file of a `direct` core turned into a set by [`with_launch`].
pub(crate) fn direct_rules(prefs: &CorePreferences, core_id: &str) -> MediaRules {
    let mut rules = MediaRules::new(MediaPolicy::Direct);
    if let Some(id) = CoreId::new(core_id) {
        rules.media_direct = prefs.media_direct(&id);
        rules.m3u_launch = m3u_launch(prefs, &id);
    }
    rules
}

/// Key of the game's options file (design v2.1-identite-disques §3.2): `GameIdentity::save_name`, the same name as
/// its SRAM, states and working copies.
pub(crate) fn game_key(identity: &GameIdentity) -> &str {
    &identity.save_name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> GameContent {
        GameContent::Files(names.iter().map(PathBuf::from).collect())
    }

    /// Mega CD (2.1): discs go to genesis_plus_gx in place, never as a media set (no copy, no extraction).
    #[test]
    fn mega_cd_discs_are_loaded_in_place() {
        let prefs = CorePreferences::builtin();
        for name in ["/g/Dune (Europe).cue", "/g/Dune.chd", "/g/Dune.iso"] {
            assert_eq!(
                content_spec("genesis_plus_gx", &files(&[name]), &prefs),
                ContentSpec::File(PathBuf::from(name)),
                "{name}"
            );
        }
    }

    #[test]
    fn puae_hands_ipf_and_lha_over_in_place() {
        let prefs = CorePreferences::builtin();
        match content_spec("puae", &files(&["/g/Zool.adf", "/g/Zool 2.adf"]), &prefs) {
            ContentSpec::Media { rules, .. } => {
                assert_eq!(rules.policy, MediaPolicy::WorkingCopy);
                assert_eq!(rules.media_direct, ["ipf", "lha"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hatari_always_gets_working_copies() {
        let prefs = CorePreferences::builtin();
        match content_spec("hatari", &files(&["/g/Nova.st"]), &prefs) {
            ContentSpec::Media { set, rules } => {
                assert_eq!(rules.policy, MediaPolicy::WorkingCopy);
                assert_eq!(set.name, "Nova");
                assert_eq!(set.images, vec![PathBuf::from("/g/Nova.st")]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unknown_cores_get_working_copies_for_floppies_only() {
        let prefs = CorePreferences::builtin();
        for name in ["hatari-old", "../x"] {
            match content_spec(name, &files(&["/g/Nova.st"]), &prefs) {
                ContentSpec::Media { rules, .. } => {
                    assert_eq!(rules.policy, MediaPolicy::WorkingCopy)
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(
            content_spec("mystery", &files(&["/g/a.sfc"]), &prefs),
            ContentSpec::File(PathBuf::from("/g/a.sfc"))
        );
        assert_eq!(
            content_spec("mystery", &files(&["/g/a.md"]), &prefs),
            ContentSpec::File(PathBuf::from("/g/a.md"))
        );
        match content_spec("hatari-old", &files(&["/g/x.m3u"]), &prefs) {
            ContentSpec::Media { rules, .. } => assert_eq!(rules.policy, MediaPolicy::WorkingCopy),
            other => panic!("{other:?}"),
        }
        for f in ["a.ide", "a.vhd", "a.gem", "a.hdf", "a.unknown"] {
            match content_spec("hatari-old", &files(&[f]), &prefs) {
                ContentSpec::Media { rules, .. } => {
                    assert_eq!(rules.policy, MediaPolicy::WorkingCopy, "{f}");
                }
                other => panic!("{other:?}"),
            }
        }
        // Known direct core: explicit policy kept.
        assert_eq!(
            content_spec("hatarib", &files(&["/g/Nova.st"]), &prefs),
            ContentSpec::File(PathBuf::from("/g/Nova.st"))
        );
    }

    #[test]
    fn consoles_keep_the_1_0_path() {
        let prefs = CorePreferences::builtin();
        assert_eq!(
            content_spec("snes9x", &files(&["/g/a.sfc"]), &prefs),
            ContentSpec::File(PathBuf::from("/g/a.sfc"))
        );
        assert_eq!(
            content_spec("hatari", &GameContent::NoGame, &prefs),
            ContentSpec::NoGame
        );
        assert_eq!(
            content_spec("hatari", &files(&[]), &prefs),
            ContentSpec::NoGame
        );
    }

    #[test]
    fn disk_sets_and_playlists() {
        let prefs = CorePreferences::builtin();
        let set = files(&[
            "/g/Game (1990)(Pub)(Disk 1 of 2).st",
            "/g/Game (1990)(Pub)(Disk 2 of 2).st",
        ]);
        match content_spec("hatarib", &set, &prefs) {
            ContentSpec::Media { set, rules } => {
                assert_eq!(rules.policy, MediaPolicy::Direct);
                assert_eq!(set.name, "Game (1990)(Pub)");
                assert_eq!(set.images.len(), 2);
            }
            other => panic!("{other:?}"),
        }
        match content_spec("hatari", &files(&["/g/Jeu.m3u8"]), &prefs) {
            ContentSpec::Media { set, .. } => {
                assert_eq!(set.playlist, Some(PathBuf::from("/g/Jeu.m3u8")));
            }
            other => panic!("{other:?}"),
        }
        match content_spec("hatari", &files(&["/g/Jeu.M3U"]), &prefs) {
            ContentSpec::Media { set, .. } => {
                assert_eq!(set.playlist, Some(PathBuf::from("/g/Jeu.M3U")));
                assert_eq!(set.name, "Jeu");
            }
            other => panic!("{other:?}"),
        }
        // Unrelated files: named after the first one.
        assert_eq!(
            media_set_name(&[PathBuf::from("/a/x.st"), PathBuf::from("/a/y.st")]),
            "x"
        );
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }

    fn policy_of(spec: &ContentSpec) -> Option<MediaPolicy> {
        match spec {
            ContentSpec::Media { rules, .. } => Some(rules.policy),
            _ => None,
        }
    }

    /// §2.4: unknown core + zip of console ROMs ⇒ 1.0 path; floppies, mixed, unknown or unreadable ⇒ copy.
    #[test]
    fn unknown_core_zip_is_inspected() {
        let prefs = CorePreferences::builtin();
        let dir = tempfile::tempdir().unwrap();
        let zip = |name: &str, entries: &[(&str, &[u8])]| {
            let path = dir.path().join(name);
            write_zip(&path, entries);
            path
        };
        let snes = zip("snes.zip", &[("Game.sfc", b"rom"), ("empty.txt", b"")]);
        assert_eq!(
            content_spec("mystery", &GameContent::Files(vec![snes.clone()]), &prefs),
            ContentSpec::File(snes)
        );
        let st = zip("st.zip", &[("disk1.st", b"disk")]);
        let mixed = zip("mixed.zip", &[("Game.sfc", b"rom"), ("disk1.st", b"disk")]);
        let unknown = zip("unknown.zip", &[("readme.xyz", b"x")]);
        let empty = zip("empty.zip", &[]);
        let broken = dir.path().join("broken.zip");
        std::fs::write(&broken, b"not a zip").unwrap();
        for path in [st, mixed, unknown, empty, broken] {
            let spec = content_spec("mystery", &GameContent::Files(vec![path.clone()]), &prefs);
            assert_eq!(
                policy_of(&spec),
                Some(MediaPolicy::WorkingCopy),
                "{}",
                path.display()
            );
        }
        // A known core never needs the inspection: its explicit policy decides.
        let st = dir.path().join("st.zip");
        assert_eq!(
            policy_of(&content_spec(
                "hatari",
                &GameContent::Files(vec![st]),
                &prefs
            )),
            Some(MediaPolicy::WorkingCopy)
        );
    }

    /// Essai CPC 5.0: a zip of floppies for cap32 is a media set (the runtime extracts the images); hatarib,
    /// console and dosbox_pure zips are unchanged.
    #[test]
    fn cap32_zip_is_a_media_set() {
        let prefs = CorePreferences::builtin();
        let dir = tempfile::tempdir().unwrap();
        let cpc = dir.path().join("Sorcery.zip");
        write_zip(&cpc, &[("Sorcery.dsk", b"disk")]);
        let two = dir.path().join("Two.zip");
        write_zip(
            &two,
            &[("Two (Disk 1).dsk", b"a"), ("Two (Disk 2).dsk", b"b")],
        );
        for (core, zip) in [("cap32", &cpc), ("cap32", &two)] {
            match content_spec(core, &GameContent::Files(vec![zip.clone()]), &prefs) {
                ContentSpec::Media { set, rules } => {
                    assert_eq!(set.images, vec![zip.clone()], "{core}");
                    assert_eq!(rules.policy, MediaPolicy::Direct, "{core}");
                }
                other => panic!("{core}: {other:?}"),
            }
        }
        // Upper-case extension too.
        let upper = dir.path().join("UPPER.ZIP");
        write_zip(&upper, &[("A.DSK", b"disk")]);
        assert!(matches!(
            content_spec("cap32", &GameContent::Files(vec![upper]), &prefs),
            ContentSpec::Media { .. }
        ));
        // hatarib reads zips itself (not checked with the real core): 1.0 path kept.
        assert_eq!(
            content_spec("hatarib", &GameContent::Files(vec![cpc.clone()]), &prefs),
            ContentSpec::File(cpc.clone())
        );
        // Working-copy computer cores were already sets.
        for core in ["puae", "puae2021", "hatari", "vice_x64", "vice_x64sc"] {
            assert_eq!(
                policy_of(&content_spec(
                    core,
                    &GameContent::Files(vec![cpc.clone()]),
                    &prefs
                )),
                Some(MediaPolicy::WorkingCopy),
                "{core}"
            );
        }
        // Console: the archive itself (the core reads zips).
        let snes = dir.path().join("Game.zip");
        write_zip(&snes, &[("Game.sfc", b"rom")]);
        assert_eq!(
            content_spec("snes9x", &GameContent::Files(vec![snes.clone()]), &prefs),
            ContentSpec::File(snes.clone())
        );
        assert_eq!(
            content_spec(
                "genesis_plus_gx",
                &GameContent::Files(vec![snes.clone()]),
                &prefs
            ),
            ContentSpec::File(snes)
        );
        // dosbox_pure reads its zips (`media_direct`: handed over in place by the runtime): unchanged.
        let dos = dir.path().join("Doom.zip");
        write_zip(&dos, &[("DOOM.EXE", b"mz")]);
        match content_spec(
            "dosbox_pure",
            &GameContent::Files(vec![dos.clone()]),
            &prefs,
        ) {
            ContentSpec::Media { set, rules } => {
                assert_eq!(set.images, vec![dos]);
                assert_eq!(rules.policy, MediaPolicy::WorkingCopy);
                assert!(rules.media_direct.iter().any(|e| e == "zip"));
            }
            other => panic!("{other:?}"),
        }
        // A bare floppy keeps the 1.0 path.
        let dsk = dir.path().join("Sorcery.dsk");
        assert_eq!(
            content_spec("cap32", &GameContent::Files(vec![dsk.clone()]), &prefs),
            ContentSpec::File(dsk)
        );
    }

    #[test]
    fn game_key_is_the_save_name() {
        let id = GameIdentity::legacy("Jeu", Path::new("/s/hatari"));
        assert_eq!(game_key(&id), "Jeu");
    }

    /// Design v5.0 §2.1-§2.2: `m3u_launch` and `save_scope` from `cores.toml`; the launch text turns a single CPC
    /// file into a set and is lower-cased for VICE.
    #[test]
    fn eight_bit_rules_and_launch_text() {
        let prefs = CorePreferences::builtin();
        assert_eq!(save_scope(&prefs, "cap32"), SaveScope::Game);
        assert_eq!(save_scope(&prefs, "vice_x64"), SaveScope::Core);
        assert_eq!(save_scope(&prefs, "nope"), SaveScope::Core);
        let cpc = content_spec("cap32", &files(&["/g/Sorcery.dsk"]), &prefs);
        assert_eq!(cpc, ContentSpec::File(PathBuf::from("/g/Sorcery.dsk")));
        let with = with_launch(
            cpc.clone(),
            Some(" RUN\"SORCERY "),
            M3uLaunch::Command,
            || direct_rules(&prefs, "cap32"),
        );
        match with {
            ContentSpec::Media { set, rules } => {
                assert_eq!(set.launch.as_deref(), Some("RUN\"SORCERY"));
                assert_eq!(set.images, vec![PathBuf::from("/g/Sorcery.dsk")]);
                assert_eq!(set.name, "Sorcery");
                assert_eq!(rules.policy, MediaPolicy::Direct);
                assert_eq!(rules.m3u_launch, M3uLaunch::Command);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            with_launch(
                cpc.clone(),
                Some("  "),
                M3uLaunch::Command,
                || unreachable!()
            ),
            cpc
        );
        assert_eq!(
            with_launch(
                ContentSpec::NoGame,
                Some("RUN"),
                M3uLaunch::Command,
                || unreachable!()
            ),
            ContentSpec::NoGame
        );
        let c64 = content_spec("vice_x64", &files(&["/g/Game.d64"]), &prefs);
        match with_launch(c64, Some("INTRO"), M3uLaunch::Program, || unreachable!()) {
            ContentSpec::Media { set, rules } => {
                assert_eq!(set.launch.as_deref(), Some("intro"));
                assert_eq!(rules.policy, MediaPolicy::WorkingCopy);
                assert_eq!(rules.m3u_launch, M3uLaunch::Program);
            }
            other => panic!("{other:?}"),
        }
        match content_spec("hatari", &files(&["/g/Nova.st"]), &prefs) {
            ContentSpec::Media { rules, .. } => assert_eq!(rules.m3u_launch, M3uLaunch::None),
            other => panic!("{other:?}"),
        }
    }
}
