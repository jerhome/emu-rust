//! Core choice for a game. Pure: no file system, no network (callers pass what is installed).
//! System keys are **libretro database names** (`"Nintendo - Game Boy"`), the same as `.info` `database`
//! fields, libretro-thumbnails and the `.rdb` files, shared with emu-library.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{CoreId, CoreInfo};

const BUILTIN: &str = include_str!("../../data/cores.toml");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreferencesFile {
    systems: BTreeMap<String, SystemEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemEntry {
    cores: Vec<String>,
}

/// Ordered core preferences per system, from the embedded `data/cores.toml`, plus user overrides
/// (`[cores.preferred]` in `config.toml`, keyed by database name), which go first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorePreferences {
    systems: BTreeMap<String, Vec<CoreId>>,
}

impl CorePreferences {
    /// Parses the embedded `data/cores.toml`; a parse error is a bug caught by a unit test.
    pub fn builtin() -> Self {
        match Self::parse(BUILTIN) {
            Ok(prefs) => prefs,
            Err(e) => {
                tracing::error!("data/cores.toml invalide : {e}");
                Self::default()
            }
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        let file: PreferencesFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut systems = BTreeMap::new();
        for (system, entry) in file.systems {
            let mut cores = Vec::new();
            for core in entry.cores {
                let id = CoreId::new(&core).ok_or_else(|| format!("id invalide : {core}"))?;
                if !cores.contains(&id) {
                    cores.push(id);
                }
            }
            systems.insert(system, cores);
        }
        Ok(Self { systems })
    }

    /// `system → core_id` from the user config; invalid ids are ignored with a warning.
    pub fn with_overrides(mut self, overrides: &BTreeMap<String, String>) -> Self {
        for (system, core) in overrides {
            let Some(id) = CoreId::new(core.trim()) else {
                tracing::warn!("[cores.preferred] {system} : identifiant de core invalide, ignoré");
                continue;
            };
            let cores = self.systems.entry(system.clone()).or_default();
            cores.retain(|c| c != &id);
            cores.insert(0, id);
        }
        self
    }

    /// Preferred cores for a system, best first (empty if unknown).
    pub fn for_system(&self, system: &str) -> &[CoreId] {
        self.systems.get(system).map_or(&[], Vec::as_slice)
    }

    /// Systems known to the preferences (for "install recommended cores").
    pub fn systems(&self) -> impl Iterator<Item = (&str, &[CoreId])> {
        self.systems.iter().map(|(s, c)| (s.as_str(), c.as_slice()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreChoice {
    Installed(CoreId),
    /// Nothing suitable installed; this one is recommended (UI offers to install it).
    Install(CoreId),
    /// No known core for this system / extension.
    Unknown,
}

/// 1. `system` (database name) known to `prefs` ⇒ first preferred core that is installed,
///    else `Install(first preferred)`.
/// 2. Otherwise, the first installed core (by id) whose `.info` lists `extension`.
/// 3. Otherwise `Unknown`.
pub fn choose_core(
    system: Option<&str>,
    extension: Option<&str>,
    installed: &[CoreId],
    infos: &BTreeMap<CoreId, CoreInfo>,
    prefs: &CorePreferences,
) -> CoreChoice {
    if let Some(preferred) = system
        .map(|s| prefs.for_system(s))
        .filter(|p| !p.is_empty())
    {
        return preferred
            .iter()
            .find(|id| installed.contains(id))
            .map_or_else(
                || CoreChoice::Install(preferred[0].clone()),
                |id| CoreChoice::Installed(id.clone()),
            );
    }
    if let Some(ext) = extension
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
    {
        let mut sorted: Vec<&CoreId> = installed.iter().collect();
        sorted.sort();
        if let Some(id) = sorted
            .into_iter()
            .find(|id| infos.get(*id).is_some_and(|i| i.extensions.contains(&ext)))
        {
            return CoreChoice::Installed(id.clone());
        }
    }
    CoreChoice::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: &str = "Nintendo - Game Boy";

    fn id(s: &str) -> CoreId {
        CoreId::new(s).unwrap()
    }

    fn info(core: &str, exts: &[&str]) -> (CoreId, CoreInfo) {
        let text = format!("supported_extensions = \"{}\"", exts.join("|"));
        (id(core), super::super::parse_info(id(core), &text).unwrap())
    }

    #[test]
    fn builtin_parses_with_expected_recommendations() {
        assert!(CorePreferences::parse(BUILTIN).is_ok());
        let prefs = CorePreferences::builtin();
        let first = |s: &str| prefs.for_system(s)[0].to_string();
        assert_eq!(first("Sega - Mega Drive - Genesis"), "genesis_plus_gx");
        assert_eq!(first("Sega - Master System - Mark III"), "genesis_plus_gx");
        assert_eq!(first("Sega - Game Gear"), "genesis_plus_gx");
        assert_eq!(
            first("Nintendo - Super Nintendo Entertainment System"),
            "snes9x"
        );
        assert_eq!(first(GB), "gambatte");
        assert_eq!(first("Nintendo - Game Boy Color"), "gambatte");
        assert_eq!(first("Nintendo - Nintendo Entertainment System"), "mesen");
        assert_eq!(prefs.systems().count(), 7);
        assert!(prefs.for_system("Unknown").is_empty());
    }

    #[test]
    fn choice() {
        let prefs = CorePreferences::builtin();
        let infos = BTreeMap::from([info("sameboy", &["gb", "gbc"]), info("zz", &["gb"])]);
        let none = BTreeMap::new();
        assert_eq!(
            choose_core(Some(GB), Some("gb"), &[], &none, &prefs),
            CoreChoice::Install(id("gambatte"))
        );
        assert_eq!(
            choose_core(Some(GB), None, &[id("mgba"), id("sameboy")], &none, &prefs),
            CoreChoice::Installed(id("sameboy"))
        );
        // Unknown system: falls back to extensions, first installed id in order.
        assert_eq!(
            choose_core(
                Some("Other"),
                Some(".GB"),
                &[id("zz"), id("sameboy")],
                &infos,
                &prefs
            ),
            CoreChoice::Installed(id("sameboy"))
        );
        assert_eq!(
            choose_core(None, Some("nes"), &[id("sameboy")], &infos, &prefs),
            CoreChoice::Unknown
        );
        assert_eq!(
            choose_core(None, None, &[], &none, &prefs),
            CoreChoice::Unknown
        );
    }

    #[test]
    fn overrides_go_first() {
        let overrides = BTreeMap::from([
            (GB.to_owned(), "sameboy".to_owned()),
            ("New - System".to_owned(), "newcore".to_owned()),
            ("Nintendo - Game Boy Color".to_owned(), "../bad".to_owned()),
        ]);
        let prefs = CorePreferences::builtin().with_overrides(&overrides);
        let names = |s: &str| -> Vec<String> {
            prefs
                .for_system(s)
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        assert_eq!(names(GB), ["sameboy", "gambatte", "mgba"]);
        assert_eq!(names("New - System"), ["newcore"]);
        assert_eq!(names("Nintendo - Game Boy Color")[0], "gambatte");
        assert_eq!(
            choose_core(Some(GB), None, &[id("gambatte")], &BTreeMap::new(), &prefs),
            CoreChoice::Installed(id("gambatte"))
        );
        assert_eq!(
            choose_core(Some(GB), None, &[], &BTreeMap::new(), &prefs),
            CoreChoice::Install(id("sameboy"))
        );
    }
}
