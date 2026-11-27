//! `[library]`, `[covers]` and `[credentials.*]` sections of `config.toml` (ADR 0014). Secrets are
//! `emu_assets::Secret`: redacted `Debug`, never logged nor shown; the file stays 0600 (ADR 0011).

use std::path::PathBuf;

use emu_assets::Secret;
use serde::{Deserialize, Serialize};

/// Current purge generation: a config below it (a missing field reads 0) is purged once at start.
pub const COVERS_MISSING_RESET: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryConfig {
    /// Scanned folders, in addition to `<data>/roms` (always scanned).
    pub roots: Vec<PathBuf>,
    /// Incremental scan when the library screen opens.
    pub scan_on_start: bool,
    /// One-shot purge generation of the `Missing` cover records (2: records written by sources that
    /// answered 404 for everything are forgotten).
    pub covers_missing_reset: u32,
    /// File names (with extension) of `megacd` games marked « CD 32X » by hand (ADR 0024 §2); compared in
    /// Unicode lower case on both sides.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cd32x: Vec<String>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            scan_on_start: true,
            covers_missing_reset: 0,
            cd32x: Vec::new(),
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CoversConfig {
    /// Downloads allowed at all (off ⇒ cache only). Off by default: no request without an explicit user choice.
    pub download: bool,
    /// `CoverSourceKind::name`s in fallback order; unknown names ignored with a warning.
    pub sources: Vec<String>,
    /// A cover not found anywhere is searched again after this many days.
    pub retry_days: u32,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for CoversConfig {
    fn default() -> Self {
        Self {
            download: false,
            sources: vec!["libretro".into(), "screenscraper".into(), "igdb".into()],
            retry_days: 30,
            extra: toml::Table::new(),
        }
    }
}

/// A source without credentials is skipped silently.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CredentialsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenscraper: Option<ScreenScraperConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub igdb: Option<IgdbConfig>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenScraperConfig {
    pub dev_id: String,
    pub dev_password: Secret,
    pub user: String,
    pub password: Secret,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IgdbConfig {
    pub client_id: String,
    pub client_secret: Secret,
}

/// `[cores]` section (ADR 0013).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CoresConfig {
    /// Alternative buildbot (`BuildbotSource::custom`); the UI warns permanently when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buildbot_url: Option<String>,
    /// Library system id (e.g. `gb`) → preferred core base name (e.g. `sameboy`).
    pub preferred: std::collections::BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

/// What a switch to an official system changes in the user's settings (ADR 0025 §10, steps a and b): the
/// definition being replaced, the official system, and the cores of the definition (the first one is the user's).
#[derive(Debug, Clone, Copy)]
pub(crate) struct SwitchTarget<'a> {
    pub custom_id: &'a str,
    pub official: &'a emu_library::SystemInfo,
    pub cores: &'a [String],
    /// First core of the official system in `cores.toml` (`None`: unknown).
    pub official_first_core: Option<&'a str>,
}

/// Bound of the keyboard profile kept for the undo (the journal header takes 16 Kio at most).
const PRIOR_PROFILE_MAX: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
struct KeyMapText {
    keys: crate::keyboard::KeyMap,
}

fn keymap_text(map: &crate::keyboard::KeyMap) -> Option<String> {
    let text = toml::to_string(&KeyMapText { keys: map.clone() }).ok()?;
    (text.len() <= PRIOR_PROFILE_MAX).then_some(text)
}

fn keymap_from_text(text: &str) -> Option<crate::keyboard::KeyMap> {
    toml::from_str::<KeyMapText>(text).ok().map(|t| t.keys)
}

impl crate::Config {
    /// `(custom key, official key)` of the profiles of each core of the definition.
    fn switch_profile_keys(target: &SwitchTarget<'_>) -> Vec<(String, String)> {
        target
            .cores
            .iter()
            .map(|core| {
                let style = crate::PadStyle::for_system(target.official, core);
                (
                    format!("{core}@{}", target.custom_id),
                    crate::profile_key_for(target.official, style, core),
                )
            })
            .collect()
    }

    /// The official profile of a core exists and differs from the user's: the switch asks which one to keep.
    pub(crate) fn switch_keys_conflict(&self, target: &SwitchTarget<'_>) -> bool {
        let keyboard = &self.input.keyboard;
        let pads = &self.input.gamepad.bindings;
        Self::switch_profile_keys(target)
            .iter()
            .any(|(custom, official)| {
                keyboard
                    .get(custom)
                    .zip(keyboard.get(official))
                    .is_some_and(|(a, b)| a != b)
                    || pads
                        .get(custom)
                        .zip(pads.get(official))
                        .is_some_and(|(a, b)| a != b)
            })
    }

    /// What the undo needs: the core preference of the official id and the keyboard profile of the user's core.
    pub(crate) fn switch_prior(
        &self,
        target: &SwitchTarget<'_>,
    ) -> (Option<Vec<String>>, Option<String>) {
        let preference = self
            .cores
            .preferred
            .get(&target.official.id)
            .map(|core| vec![core.clone()]);
        let profile = Self::switch_profile_keys(target)
            .first()
            .and_then(|(_, official)| self.input.keyboard.get(official))
            .and_then(keymap_text);
        (preference, profile)
    }

    /// Steps (a) keys and (b) core of a switch. Idempotent. The official profile is created from the user's one
    /// when it does not exist; when both exist and differ, `keys` decides (`KeepUser` overwrites). The user's core
    /// stays first for the official system when it is not the one of `cores.toml`.
    pub(crate) fn apply_switch(
        &mut self,
        target: &SwitchTarget<'_>,
        keys: emu_library::KeysChoice,
    ) {
        let overwrite = matches!(keys, emu_library::KeysChoice::KeepUser);
        for (custom, official) in Self::switch_profile_keys(target) {
            if let Some(mine) = self.input.keyboard.get(&custom).cloned()
                && (overwrite || !self.input.keyboard.contains_key(&official))
            {
                self.input.keyboard.insert(official.clone(), mine);
            }
            if let Some(mine) = self.input.gamepad.bindings.get(&custom).cloned()
                && (overwrite || !self.input.gamepad.bindings.contains_key(&official))
            {
                self.input.gamepad.bindings.insert(official, mine);
            }
        }
        if let (Some(user), Some(first)) = (target.cores.first(), target.official_first_core)
            && user != first
        {
            self.cores
                .preferred
                .insert(target.official.id.clone(), user.clone());
        }
    }

    /// What the undo of a switch gives back: the core preference and the keyboard profile of before.
    pub(crate) fn undo_switch(
        &mut self,
        target: &SwitchTarget<'_>,
        prior_preference: Option<&[String]>,
        prior_profile: Option<&str>,
    ) {
        match prior_preference.and_then(<[String]>::first) {
            Some(core) => {
                self.cores
                    .preferred
                    .insert(target.official.id.clone(), core.clone());
            }
            None => {
                // Only the preference the switch wrote (the user's core), never one chosen since.
                if target.cores.first() == self.cores.preferred.get(&target.official.id) {
                    self.cores.preferred.remove(&target.official.id);
                }
            }
        }
        let Some((custom, official)) = Self::switch_profile_keys(target).into_iter().next() else {
            return;
        };
        match prior_profile {
            Some(text) => {
                if let Some(map) = keymap_from_text(text) {
                    self.input.keyboard.insert(official, map);
                }
            }
            None => {
                // The official profile was absent: remove the copy, not a profile the user changed since.
                if self.input.keyboard.get(&official) == self.input.keyboard.get(&custom) {
                    self.input.keyboard.remove(&official);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    const SECRET_TEXT: &str = r#"
[library]
roots = ["/roms"]

[credentials.screenscraper]
dev_id = "devid"
dev_password = "hunter2-dev"
user = "me"
password = "hunter2-user"

[credentials.igdb]
client_id = "cid"
client_secret = "hunter2-igdb"

[cores]
buildbot_url = "https://example.org/b"
[cores.preferred]
gb = "sameboy"
"#;

    #[test]
    fn debug_never_shows_secrets() {
        let cfg: Config = toml::from_str(SECRET_TEXT).unwrap();
        let shown = format!("{cfg:?} {:#?}", cfg.credentials);
        assert!(!shown.contains("hunter2"));
        assert!(shown.contains("Secret(***)"));
        assert_eq!(
            cfg.credentials.igdb.unwrap().client_secret.expose(),
            "hunter2-igdb"
        );
    }

    #[test]
    fn parse_error_does_not_echo_secrets() {
        let bad = "[credentials.igdb]\nclient_secret = 12345678secret\n";
        let err = toml::from_str::<Config>(bad).unwrap_err();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, bad).unwrap();
        let shown = Config::load(&path).unwrap_err().to_string();
        assert!(!shown.contains("12345678secret"), "{shown}");
        let _ = err;
    }

    #[test]
    fn round_trip_and_private_file() {
        let cfg: Config = toml::from_str(SECRET_TEXT).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        cfg.save(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let back = Config::load(&path).unwrap();
        assert_eq!(back.credentials, cfg.credentials);
        assert_eq!(back.library.roots, cfg.library.roots);
        assert_eq!(back.cores, cfg.cores);
        assert_eq!(back.cores.preferred["gb"], "sameboy");
    }

    #[test]
    fn defaults_without_sections() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.library.scan_on_start);
        assert!(!cfg.covers.download);
        assert_eq!(cfg.covers.sources.len(), 3);
        assert!(cfg.credentials.screenscraper.is_none());
        assert_eq!(cfg.cores, CoresConfig::default());
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(!text.contains("credentials.screenscraper"));
    }

    #[test]
    fn data_dirs_layout() {
        let d = crate::DataDirs::under(std::path::Path::new("/d"));
        assert_eq!(d.library_db, std::path::Path::new("/d/library.sqlite"));
        assert!(d.cores.starts_with("/d/cores"));
        assert_eq!(d.database, std::path::Path::new("/d/database/no-intro"));
    }

    mod switch {
        use emu_library::{Catalog, KeysChoice, SystemInfo};

        use super::*;
        use crate::keyboard::KeyMap;

        fn snes() -> SystemInfo {
            Catalog::official().system("snes").unwrap().clone()
        }

        fn map(key: winit::keyboard::KeyCode) -> KeyMap {
            let mut m = KeyMap::default();
            m.bind(key, 0, emu_runtime::JoypadButton::A);
            m
        }

        fn target<'a>(official: &'a SystemInfo, cores: &'a [String]) -> SwitchTarget<'a> {
            SwitchTarget {
                custom_id: "mysnes",
                official,
                cores,
                official_first_core: Some("snes9x"),
            }
        }

        #[test]
        fn the_official_profile_is_created_from_the_users_one_when_it_is_absent() {
            let (official, cores) = (snes(), vec!["snes9x".to_owned()]);
            let mut cfg = Config::default();
            cfg.set_keymap("snes9x@mysnes", map(winit::keyboard::KeyCode::KeyQ));
            assert!(!cfg.switch_keys_conflict(&target(&official, &cores)));
            cfg.apply_switch(&target(&official, &cores), KeysChoice::Copy);
            assert_eq!(
                cfg.input.keyboard.get("snes9x"),
                Some(&map(winit::keyboard::KeyCode::KeyQ))
            );
        }

        #[test]
        fn a_different_official_profile_is_a_conflict_and_the_choice_decides() {
            let (official, cores) = (snes(), vec!["snes9x".to_owned()]);
            let (mine, theirs) = (
                map(winit::keyboard::KeyCode::KeyQ),
                map(winit::keyboard::KeyCode::KeyW),
            );
            let mut cfg = Config::default();
            cfg.set_keymap("snes9x@mysnes", mine.clone());
            cfg.set_keymap("snes9x", theirs.clone());
            assert!(cfg.switch_keys_conflict(&target(&official, &cores)));
            let mut official_wins = cfg.clone();
            official_wins.apply_switch(&target(&official, &cores), KeysChoice::KeepOfficial);
            assert_eq!(official_wins.input.keyboard.get("snes9x"), Some(&theirs));
            cfg.apply_switch(&target(&official, &cores), KeysChoice::KeepUser);
            assert_eq!(cfg.input.keyboard.get("snes9x"), Some(&mine));
        }

        #[test]
        fn the_users_core_stays_first_only_when_it_is_not_the_official_one() {
            let official = snes();
            let mut cfg = Config::default();
            let same = vec!["snes9x".to_owned()];
            cfg.apply_switch(&target(&official, &same), KeysChoice::Copy);
            assert!(cfg.cores.preferred.is_empty());
            let other = vec!["bsnes".to_owned(), "snes9x".to_owned()];
            cfg.apply_switch(&target(&official, &other), KeysChoice::Copy);
            assert_eq!(cfg.cores.preferred["snes"], "bsnes");
            // Idempotent: a resumed switch changes nothing more.
            let again = cfg.clone();
            cfg.apply_switch(&target(&official, &other), KeysChoice::Copy);
            assert_eq!(cfg, again);
        }

        #[test]
        fn the_undo_gives_back_the_preference_and_the_profile_of_before() {
            let (official, cores) = (snes(), vec!["bsnes".to_owned()]);
            let theirs = map(winit::keyboard::KeyCode::KeyW);
            let mut cfg = Config::default();
            cfg.set_keymap("bsnes@mysnes", map(winit::keyboard::KeyCode::KeyQ));
            cfg.set_keymap("bsnes", theirs.clone());
            cfg.cores.preferred.insert("snes".into(), "snes9x".into());
            let t = target(&official, &cores);
            let (preference, profile) = cfg.switch_prior(&t);
            assert_eq!(preference.as_deref(), Some(&["snes9x".to_owned()][..]));
            assert!(profile.is_some());
            cfg.apply_switch(&t, KeysChoice::KeepUser);
            assert_eq!(cfg.cores.preferred["snes"], "bsnes");
            cfg.undo_switch(&t, preference.as_deref(), profile.as_deref());
            assert_eq!(cfg.cores.preferred["snes"], "snes9x");
            assert_eq!(cfg.input.keyboard.get("bsnes"), Some(&theirs));
        }

        #[test]
        fn the_undo_removes_a_copied_profile_but_not_one_changed_since() {
            let (official, cores) = (snes(), vec!["bsnes".to_owned()]);
            let t = target(&official, &cores);
            let mut cfg = Config::default();
            cfg.set_keymap("bsnes@mysnes", map(winit::keyboard::KeyCode::KeyQ));
            cfg.apply_switch(&t, KeysChoice::Copy);
            let mut changed = cfg.clone();
            changed.set_keymap("bsnes", map(winit::keyboard::KeyCode::KeyE));
            cfg.undo_switch(&t, None, None);
            assert!(!cfg.input.keyboard.contains_key("bsnes"));
            assert!(!cfg.cores.preferred.contains_key("snes"));
            changed.undo_switch(&t, None, None);
            assert!(changed.input.keyboard.contains_key("bsnes"));
        }
    }
}
