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
}
