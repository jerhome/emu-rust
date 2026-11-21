//! Pure construction of the rows shown by the cores screen (no I/O, unit-tested).

use std::collections::{BTreeMap, BTreeSet};

use emu_assets::cores::{
    BuildDate, CoreId, CoreInfo, CorePreferences, FirmwareState, FirmwareStatus, InstalledCore,
    Origin, RemoteCatalog,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Scope {
    /// Cores named by `cores.toml` (plus anything installed).
    #[default]
    Release,
    /// The whole catalog.
    All,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RowFilter {
    pub scope: Scope,
    /// Case-insensitive substring of the id or display name.
    pub search: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreState {
    NotInstalled,
    UpToDate,
    UpdateAvailable,
    /// Hand-placed file: usable, never verified nor updated.
    Manual,
    /// Hash differs from the recorded one: never loaded.
    Tampered,
}

/// Checks done outside `rows` (they hash files).
#[derive(Debug, Default)]
pub(crate) struct Checks {
    pub tampered: BTreeSet<CoreId>,
    pub firmware: BTreeMap<CoreId, Vec<FirmwareStatus>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoreRow {
    pub id: CoreId,
    pub name: String,
    pub systems: Vec<String>,
    pub license: Option<String>,
    pub state: CoreState,
    pub installed: Option<BuildDate>,
    pub available: Option<BuildDate>,
    pub pinned: bool,
    pub has_previous: bool,
    pub in_use: bool,
    pub recommended: bool,
    pub firmware_missing: usize,
    pub firmware_wrong: usize,
    pub downloadable: bool,
}

impl CoreRow {
    pub fn is_installed(&self) -> bool {
        self.state != CoreState::NotInstalled
    }

    pub fn can_update(&self) -> bool {
        self.state == CoreState::UpdateAvailable && !self.pinned && !self.in_use
    }
}

/// Library system names for `.info` database names (`Nintendo - Game Boy` → « Game Boy »).
fn system_names(info: Option<&CoreInfo>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for db in info.map_or(&[][..], |i| i.databases.as_slice()) {
        let name = emu_library::systems()
            .iter()
            .find(|s| &s.database == db)
            .map_or_else(|| db.clone(), |s| s.name.clone());
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

pub(crate) fn rows(
    installed: &[InstalledCore],
    catalog: Option<&RemoteCatalog>,
    infos: &BTreeMap<CoreId, CoreInfo>,
    prefs: &CorePreferences,
    filter: &RowFilter,
    in_use: Option<&CoreId>,
    checks: &Checks,
) -> Vec<CoreRow> {
    let recommended: BTreeSet<&CoreId> = prefs
        .systems()
        .flat_map(|(_, cores)| cores.iter().take(1))
        .collect();
    let wanted: BTreeSet<&CoreId> = prefs
        .systems()
        .flat_map(|(_, cores)| cores.iter())
        .collect();
    let by_id: BTreeMap<&CoreId, &InstalledCore> = installed.iter().map(|c| (&c.id, c)).collect();

    let mut ids: BTreeSet<&CoreId> = by_id.keys().copied().collect();
    match filter.scope {
        Scope::Release => ids.extend(wanted.iter().copied()),
        Scope::All => {
            ids.extend(wanted.iter().copied());
            if let Some(c) = catalog {
                ids.extend(c.cores.keys());
            }
        }
    }

    let needle = filter.search.trim().to_lowercase();
    let mut out: Vec<CoreRow> = ids
        .into_iter()
        .filter_map(|id| {
            let info = infos.get(id);
            let name = info.map_or_else(
                || id.as_str().to_owned(),
                |i| crate::ui::text::sanitize_core_text(&i.display_name),
            );
            if !needle.is_empty()
                && !name.to_lowercase().contains(&needle)
                && !id.as_str().to_lowercase().contains(&needle)
            {
                return None;
            }
            let remote = catalog.and_then(|c| c.cores.get(id));
            let local = by_id.get(id);
            let (state, installed_date, pinned) = match local.map(|c| &c.origin) {
                None => (CoreState::NotInstalled, None, false),
                Some(Origin::Manual) => (CoreState::Manual, None, false),
                Some(Origin::Buildbot {
                    date,
                    crc32,
                    pinned,
                    ..
                }) => {
                    let state = if checks.tampered.contains(id) {
                        CoreState::Tampered
                    } else if remote.is_some_and(|r| r.crc32 != *crc32) {
                        CoreState::UpdateAvailable
                    } else {
                        CoreState::UpToDate
                    };
                    (state, Some(*date), *pinned)
                }
            };
            let (mut missing, mut wrong) = (0, 0);
            for f in checks.firmware.get(id).into_iter().flatten() {
                if f.spec.optional {
                    continue;
                }
                match f.state {
                    FirmwareState::Missing => missing += 1,
                    FirmwareState::WrongHash { .. } => wrong += 1,
                    _ => {}
                }
            }
            Some(CoreRow {
                id: id.clone(),
                name,
                systems: system_names(info),
                license: info.and_then(|i| i.license.clone()),
                state,
                installed: installed_date,
                available: remote.map(|r| r.date),
                pinned,
                has_previous: local.is_some_and(|c| c.has_previous),
                in_use: in_use == Some(id),
                recommended: recommended.contains(id),
                firmware_missing: missing,
                firmware_wrong: wrong,
                downloadable: remote.is_some(),
            })
        })
        .collect();
    // Installed first, then recommended, then by name.
    out.sort_by(|a, b| {
        b.is_installed()
            .cmp(&a.is_installed())
            .then(b.recommended.cmp(&a.recommended))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// Cores to install for "install recommended cores": the first core of each system, not installed yet.
pub(crate) fn recommended_missing(
    prefs: &CorePreferences,
    installed: &[InstalledCore],
) -> Vec<CoreId> {
    let mut out: Vec<CoreId> = Vec::new();
    for (_, cores) in prefs.systems() {
        if let Some(first) = cores.first()
            && !out.contains(first)
            && !installed.iter().any(|c| &c.id == first)
        {
            out.push(first.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_assets::cores::{RemoteCore, parse_info};
    use std::path::PathBuf;

    fn id(s: &str) -> CoreId {
        CoreId::new(s).unwrap()
    }

    fn date(d: u8) -> BuildDate {
        BuildDate::parse(&format!("2026-11-{d:02}")).unwrap()
    }

    fn managed(name: &str, crc: u32, pinned: bool) -> InstalledCore {
        InstalledCore {
            id: id(name),
            path: PathBuf::from(format!("{name}_libretro.so")),
            origin: Origin::Buildbot {
                url: String::new(),
                date: date(1),
                crc32: crc,
                sha256: [0; 32],
                size: 1,
                installed_unix: 0,
                pinned,
            },
            has_previous: true,
        }
    }

    fn remote(name: &str, crc: u32) -> RemoteCore {
        RemoteCore {
            id: id(name),
            file_name: format!("{name}_libretro.so.zip"),
            date: date(5),
            crc32: crc,
        }
    }

    fn catalog(cores: &[RemoteCore]) -> RemoteCatalog {
        RemoteCatalog {
            fetched_unix: 1,
            cores: cores.iter().map(|c| (c.id.clone(), c.clone())).collect(),
        }
    }

    fn find<'a>(rows: &'a [CoreRow], name: &str) -> &'a CoreRow {
        rows.iter().find(|r| r.id.as_str() == name).unwrap()
    }

    fn default_rows(
        installed: &[InstalledCore],
        cat: Option<&RemoteCatalog>,
        filter: &RowFilter,
        in_use: Option<&CoreId>,
        checks: &Checks,
    ) -> Vec<CoreRow> {
        rows(
            installed,
            cat,
            &BTreeMap::new(),
            &CorePreferences::builtin(),
            filter,
            in_use,
            checks,
        )
    }

    #[test]
    fn states_follow_origin_crc_and_checks() {
        let installed = [
            managed("gambatte", 1, false),
            managed("snes9x", 1, true),
            managed("mesen", 7, false),
            InstalledCore {
                id: id("handmade"),
                path: PathBuf::new(),
                origin: Origin::Manual,
                has_previous: false,
            },
        ];
        let cat = catalog(&[
            remote("gambatte", 1),
            remote("snes9x", 2),
            remote("mesen", 8),
        ]);
        let mut checks = Checks::default();
        checks.tampered.insert(id("mesen"));
        let r = default_rows(&installed, Some(&cat), &RowFilter::default(), None, &checks);
        assert_eq!(find(&r, "gambatte").state, CoreState::UpToDate);
        let s = find(&r, "snes9x");
        assert_eq!(s.state, CoreState::UpdateAvailable);
        assert!(s.pinned && !s.can_update());
        assert_eq!(find(&r, "mesen").state, CoreState::Tampered);
        assert_eq!(find(&r, "handmade").state, CoreState::Manual);
        assert_eq!(find(&r, "genesis_plus_gx").state, CoreState::NotInstalled);
        assert!(find(&r, "gambatte").recommended);
        assert!(!find(&r, "picodrive").recommended);
    }

    #[test]
    fn without_catalog_nothing_is_outdated() {
        let r = default_rows(
            &[managed("gambatte", 1, false)],
            None,
            &RowFilter::default(),
            None,
            &Checks::default(),
        );
        assert_eq!(find(&r, "gambatte").state, CoreState::UpToDate);
        assert!(!find(&r, "gambatte").downloadable);
    }

    #[test]
    fn scope_and_search_filter() {
        let cat = catalog(&[remote("gambatte", 1), remote("zzz_exotic", 1)]);
        let release = default_rows(
            &[],
            Some(&cat),
            &RowFilter::default(),
            None,
            &Checks::default(),
        );
        assert!(release.iter().all(|r| r.id.as_str() != "zzz_exotic"));
        let all = RowFilter {
            scope: Scope::All,
            search: String::new(),
        };
        let r = default_rows(&[], Some(&cat), &all, None, &Checks::default());
        assert!(r.iter().any(|r| r.id.as_str() == "zzz_exotic"));
        let search = RowFilter {
            scope: Scope::All,
            search: " EXOT ".into(),
        };
        let r = default_rows(&[], Some(&cat), &search, None, &Checks::default());
        assert_eq!(r.len(), 1);
        // Installed cores are always listed, even outside the 1.0 scope.
        let r = default_rows(
            &[managed("zzz_exotic", 1, false)],
            None,
            &RowFilter::default(),
            None,
            &Checks::default(),
        );
        assert_eq!(r[0].id.as_str(), "zzz_exotic");
    }

    #[test]
    fn in_use_and_ordering() {
        let installed = [managed("snes9x", 1, false)];
        let used = id("snes9x");
        let r = default_rows(
            &installed,
            None,
            &RowFilter::default(),
            Some(&used),
            &Checks::default(),
        );
        assert_eq!(r[0].id.as_str(), "snes9x");
        assert!(r[0].in_use && !r[0].can_update());
        assert!(!find(&r, "gambatte").in_use);
    }

    #[test]
    fn names_systems_and_firmware_summary() {
        let info = parse_info(
            id("gambatte"),
            "display_name = \"Nintendo - Game Boy / Color (Gambatte)\"\nlicense = \"GPLv2\"\n\
             database = \"Nintendo - Game Boy|Nintendo - Game Boy Color\"\n\
             firmware_count = 1\nfirmware0_desc = \"bios\"\nfirmware0_path = \"gb_bios.bin\"\n\
             firmware0_opt = \"false\"\n",
        )
        .unwrap();
        let infos = BTreeMap::from([(id("gambatte"), info.clone())]);
        let mut checks = Checks::default();
        checks.firmware.insert(
            id("gambatte"),
            vec![FirmwareStatus {
                spec: info.firmware[0].clone(),
                state: FirmwareState::Missing,
            }],
        );
        let r = rows(
            &[managed("gambatte", 1, false)],
            None,
            &infos,
            &CorePreferences::builtin(),
            &RowFilter::default(),
            None,
            &checks,
        );
        let g = find(&r, "gambatte");
        assert_eq!(g.name, "Nintendo - Game Boy / Color (Gambatte)");
        assert_eq!(g.license.as_deref(), Some("GPLv2"));
        assert_eq!(g.systems.len(), 2);
        assert_eq!(g.firmware_missing, usize::from(!info.firmware[0].optional));
    }

    #[test]
    fn recommended_is_first_core_per_system_not_installed() {
        let prefs = CorePreferences::builtin();
        let all = recommended_missing(&prefs, &[]);
        let names: Vec<&str> = all.iter().map(CoreId::as_str).collect();
        // Mega CD (2.1) shares genesis_plus_gx with the Mega Drive; the Amiga (3.0) adds puae, the PC (4.0)
        // dosbox_pure.
        let expected = [
            "genesis_plus_gx",
            "snes9x",
            "gambatte",
            "mesen",
            "hatari",
            "puae",
            "dosbox_pure",
        ];
        for expected in expected {
            assert!(names.contains(&expected), "{names:?}");
        }
        assert_eq!(names.len(), expected.len());
        let some = recommended_missing(&prefs, &[managed("snes9x", 1, false)]);
        assert_eq!(some.len(), expected.len() - 1);
    }
}
