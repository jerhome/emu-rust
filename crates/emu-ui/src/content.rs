//! From the files of a game to the `ContentSpec` the runtime loads (ADR 0015 §5). Pure, tested.

use std::path::{Path, PathBuf};

use emu_assets::cores::{CoreId, CoreMediaPolicy, CorePreferences};
use emu_runtime::{ContentSpec, GameIdentity, MediaPolicy, MediaRules, MediaSet};

/// What to boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameContent {
    /// One file (1.0), the disks of one game in order, or one `.m3u`.
    Files(Vec<PathBuf>),
    /// No content (`SET_SUPPORT_NO_GAME`): the computer's desktop.
    NoGame,
}

impl GameContent {
    /// First file, if any (pad style detection, logs).
    pub fn first(&self) -> Option<&Path> {
        match self {
            Self::Files(f) => f.first().map(PathBuf::as_path),
            Self::NoGame => None,
        }
    }
}

fn is_m3u(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u"))
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

/// §2.4 (design v2.1-identite-disques): a `.zip` whose retained entries (non-empty files) all have a console
/// extension is a console ROM archive. Only the central directory is read; an unreadable or empty zip is not.
fn is_console_zip(path: &Path) -> bool {
    let is_zip = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    is_zip
        && emu_runtime::archive_entry_names(path).is_ok_and(|names| {
            !names.is_empty() && names.iter().all(|n| is_console_file(Path::new(n)))
        })
}

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
/// `File` for a single file of a `direct` core (1.0 behaviour, zips included); `Media` for several disks, a
/// `.m3u`, or any content of a `working_copy` core (hatari writes into its disks: the user's file must never be
/// handed over, even alone); `NoGame` without files.
pub fn content_spec(core_id: &str, content: &GameContent, prefs: &CorePreferences) -> ContentSpec {
    let files = match content {
        GameContent::NoGame => return ContentSpec::NoGame,
        GameContent::Files(f) if f.is_empty() => return ContentSpec::NoGame,
        GameContent::Files(f) => f,
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
    }
    if files.len() == 1 && is_m3u(&files[0]) {
        return ContentSpec::Media {
            set: MediaSet {
                name: stem(&files[0]),
                images: Vec::new(),
                playlist: Some(files[0].clone()),
            },
            rules,
        };
    }
    if files.len() == 1 && policy == MediaPolicy::Direct {
        return ContentSpec::File(files[0].clone());
    }
    ContentSpec::Media {
        set: MediaSet {
            name: media_set_name(files),
            images: files.clone(),
            playlist: None,
        },
        rules,
    }
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

    #[test]
    fn game_key_is_the_save_name() {
        let id = GameIdentity::legacy("Jeu", Path::new("/s/hatari"));
        assert_eq!(game_key(&id), "Jeu");
    }
}
