//! The machines a core offers, the rows of the « add » view, and the definitions written from a `.info` and a
//! probe (ADR 0025 §4, §9). Pure: no I/O, no catalog publication.

use std::collections::BTreeMap;

use emu_assets::cores::{CoreId, CoreInfo, CorePreferences};
use emu_library::{
    Catalog, CustomSystemDraft, DefinitionOrigin, MachineTier, cartridge_extensions, known_tier,
    normalize_name, proposed_id,
};
use emu_runtime::DryRunReport;

use super::{Addability, addability_with};

/// What can be done with one machine (one `database` name) of a core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MachineState {
    Available,
    /// An official system already has this machine: not offered.
    AlreadySupported,
    /// A definition already exists.
    AlreadyAdded,
    /// A disc machine: « prévu en 8.4 ».
    DiscPlanned,
    /// An arcade machine: « prévu en 8.6 ».
    ArcadePlanned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Machine {
    /// libretro database name; `None` for the 31 cores that name none (one machine, from the core's name).
    pub database: Option<String>,
    pub name: String,
    pub state: MachineState,
}

/// One core of the list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AddRow {
    pub core: CoreId,
    pub name: String,
    pub verdict: Addability,
    pub machines: Vec<Machine>,
    pub license: Option<String>,
}

/// The machines of `info`, in the order of the `.info`.
fn machines_of(info: &CoreInfo, catalog: &Catalog) -> Vec<Machine> {
    if info.databases.is_empty() {
        let raw = info.system_name.as_deref().unwrap_or(&info.display_name);
        let added = catalog.customs().any(|c| {
            c.origin.core == info.id.as_str()
                && catalog
                    .system(&c.id)
                    .is_some_and(|s| s.libretro_name().is_none())
        });
        return vec![Machine {
            database: None,
            name: normalize_name(raw),
            state: if added {
                MachineState::AlreadyAdded
            } else {
                MachineState::Available
            },
        }];
    }
    let has_database = |database: &str, custom: bool| {
        catalog
            .systems()
            .iter()
            .chain(catalog.masked_official())
            .any(|s| s.is_custom() == custom && s.libretro_name() == Some(database))
    };
    info.databases
        .iter()
        .map(|database| {
            let state = if has_database(database, false) {
                MachineState::AlreadySupported
            } else if has_database(database, true) {
                MachineState::AlreadyAdded
            } else {
                match known_tier(database) {
                    Some(MachineTier::Disc) => MachineState::DiscPlanned,
                    Some(MachineTier::Arcade) => MachineState::ArcadePlanned,
                    _ => MachineState::Available,
                }
            };
            Machine {
                database: Some(database.clone()),
                name: normalize_name(database),
                state,
            }
        })
        .collect()
}

fn is_emulator(info: &CoreInfo) -> bool {
    info.categories
        .iter()
        .any(|c| c.eq_ignore_ascii_case("emulator"))
}

/// The rows of the « add » view. `downloadable` tells whether the buildbot offers the core on this platform (or it
/// is installed). Default view: emulators that are not experimental; `show_all`: every core that `addability`
/// accepts, and for a non-emulator nothing greyed. A core whose machines are all already supported is left out.
/// `search` matches the core, the system name and the machines, ignoring case.
pub(crate) fn add_rows(
    infos: &BTreeMap<CoreId, CoreInfo>,
    downloadable: &dyn Fn(&CoreId) -> bool,
    catalog: &Catalog,
    prefs: &CorePreferences,
    show_all: bool,
    search: &str,
) -> Vec<AddRow> {
    let needle = search.trim().to_lowercase();
    let mut rows: Vec<AddRow> = infos
        .values()
        .filter(|info| downloadable(&info.id))
        .filter_map(|info| {
            let verdict = addability_with(info, prefs);
            if verdict == Addability::NotProposed {
                return None;
            }
            let wanted = if show_all {
                is_emulator(info) || matches!(verdict, Addability::Ok { .. })
            } else {
                is_emulator(info) && !info.experimental
            };
            if !wanted {
                return None;
            }
            let machines = machines_of(info, catalog);
            if machines
                .iter()
                .all(|m| m.state == MachineState::AlreadySupported)
            {
                return None;
            }
            let matches = needle.is_empty() || {
                let mut texts = vec![
                    info.display_name.as_str(),
                    info.id.as_str(),
                    info.system_name.as_deref().unwrap_or_default(),
                ];
                texts.extend(machines.iter().map(|m| m.name.as_str()));
                texts.iter().any(|t| t.to_lowercase().contains(&needle))
            };
            matches.then(|| AddRow {
                core: info.id.clone(),
                name: info.display_name.clone(),
                verdict,
                machines,
                license: info.license.clone(),
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.core.cmp(&b.core))
    });
    rows
}

/// Extensions of a new definition: the probe's list when the core gave one (it replaces the `.info`, ADR 0025
/// §4), else the `.info`'s; containers, playlists and disc formats removed.
fn extensions_for(info: &CoreInfo, report: Option<&DryRunReport>) -> Vec<String> {
    let raw: &[String] = match report {
        Some(r) if !r.extensions.is_empty() => &r.extensions,
        _ => &info.extensions,
    };
    cartridge_extensions(raw)
}

/// One definition per chosen machine; the ids come from `proposed_id` against the systems of `catalog` and the
/// ids of the same batch. `now_unix` and `created_by` fill the origin.
pub(crate) fn drafts_for(
    info: &CoreInfo,
    chosen: &[Machine],
    report: Option<&DryRunReport>,
    catalog: &Catalog,
    now_unix: u64,
    created_by: &str,
) -> Vec<CustomSystemDraft> {
    let extensions = extensions_for(info, report);
    let mut ids: Vec<String> = Vec::new();
    let mut drafts = Vec::new();
    for machine in chosen {
        let taken = |id: &str| catalog.system(id).is_some() || ids.iter().any(|i| i == id);
        let id = proposed_id(machine.database.as_deref(), &machine.name, &taken);
        ids.push(id.clone());
        drafts.push(CustomSystemDraft {
            id,
            name: machine.name.clone(),
            database: machine.database.clone(),
            extensions: extensions.clone(),
            dats: Vec::new(),
            screenscraper: None,
            igdb: None,
            cores: vec![info.id.to_string()],
            ignored_firmware: Vec::new(),
            origin: DefinitionOrigin {
                core: info.id.to_string(),
                info_name: info.system_name.clone(),
                created_by: created_by.to_owned(),
                created_unix: now_unix,
            },
        });
    }
    drafts
}

#[cfg(test)]
mod tests {
    use emu_assets::cores::{HwApi, HwApiKind, RenderNeed};

    use super::*;

    fn info(id: &str, extensions: &[&str], databases: &[&str]) -> CoreInfo {
        CoreInfo {
            id: CoreId::new(id).unwrap(),
            display_name: format!("Machine ({id})"),
            core_name: None,
            version: None,
            license: Some("MIT".to_owned()),
            extensions: extensions.iter().map(|s| (*s).to_owned()).collect(),
            databases: databases.iter().map(|s| (*s).to_owned()).collect(),
            firmware: Vec::new(),
            description: None,
            system_name: None,
            categories: vec!["Emulator".to_owned()],
            experimental: false,
            render: RenderNeed::Software,
            supports_no_game: None,
            needs_fullpath: None,
            keyboard_focus: None,
            savestate: None,
        }
    }

    fn report(extensions: &[&str]) -> DryRunReport {
        DryRunReport {
            outcome: emu_runtime::DryRunOutcome::Probed,
            frames_run: 0,
            library_name: String::new(),
            library_version: String::new(),
            extensions: extensions.iter().map(|s| (*s).to_owned()).collect(),
            need_fullpath: false,
            supports_no_game: false,
            hw_render: None,
            descriptors: Vec::new(),
        }
    }

    fn rows_of(list: Vec<CoreInfo>, show_all: bool, search: &str) -> Vec<AddRow> {
        let infos: BTreeMap<CoreId, CoreInfo> =
            list.into_iter().map(|i| (i.id.clone(), i)).collect();
        add_rows(
            &infos,
            &|_| true,
            &Catalog::official(),
            &CorePreferences::builtin(),
            show_all,
            search,
        )
    }

    #[test]
    fn a_core_of_an_official_machine_is_not_offered() {
        let rows = rows_of(
            vec![
                info(
                    "snes9x_like",
                    &["sfc"],
                    &["Nintendo - Super Nintendo Entertainment System"],
                ),
                info("stella_like", &["a26", "bin"], &["Atari - 2600"]),
            ],
            false,
            "",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].core.as_str(), "stella_like");
        assert_eq!(rows[0].machines[0].state, MachineState::Available);
    }

    #[test]
    fn experimental_cores_and_non_emulators_need_show_all() {
        let mut experimental = info("exp_core", &["xyz"], &[]);
        experimental.experimental = true;
        let mut game = info("game_core", &["xyz"], &[]);
        game.categories = vec!["Game".to_owned()];
        let list = vec![experimental, game, info("plain_core", &["xyz"], &[])];
        let ids = |rows: Vec<AddRow>| -> Vec<String> {
            rows.iter().map(|r| r.core.to_string()).collect()
        };
        assert_eq!(ids(rows_of(list.clone(), false, "")), ["plain_core"]);
        assert_eq!(
            ids(rows_of(list, true, "")),
            ["exp_core", "game_core", "plain_core"]
        );
    }

    #[test]
    fn greyed_cores_keep_their_tier_and_a_non_emulator_is_never_greyed() {
        let mut hardware = info("hw_core", &["a26"], &["Atari - 2600"]);
        hardware.render = RenderNeed::Hardware(vec![HwApi {
            kind: HwApiKind::OpenGlCore,
            min_version: None,
        }]);
        let disc = info("cd_core", &["cue", "chd"], &["Some - Fictional CD"]);
        let mut engine = info("engine_core", &["cue"], &[]);
        engine.categories = vec!["Game".to_owned()];
        let rows = rows_of(vec![hardware, disc, engine], true, "");
        let verdicts: Vec<(String, Addability)> = rows
            .iter()
            .map(|r| (r.core.to_string(), r.verdict.clone()))
            .collect();
        assert_eq!(
            verdicts,
            [
                ("cd_core".to_owned(), Addability::DiscOnly),
                ("hw_core".to_owned(), Addability::HardwareRender),
            ]
        );
    }

    #[test]
    fn a_disc_machine_of_a_mixed_core_is_greyed_and_the_cartridge_one_is_not() {
        let rows = rows_of(
            vec![info(
                "mixed_core",
                &["pce", "cue"],
                &[
                    "NEC - PC Engine - TurboGrafx 16",
                    "NEC - PC Engine CD - TurboGrafx-CD",
                ],
            )],
            false,
            "",
        );
        let states: Vec<MachineState> = rows[0].machines.iter().map(|m| m.state).collect();
        assert_eq!(states, [MachineState::Available, MachineState::DiscPlanned]);
    }

    #[test]
    fn search_matches_the_machine_and_ignores_case() {
        let list = vec![
            info("a_core", &["xyz"], &["Acme - Fictional Console"]),
            info("b_core", &["xyz"], &["Other - Thing"]),
        ];
        let rows = rows_of(list, false, " FICTIONAL ");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].core.as_str(), "a_core");
    }

    #[test]
    fn a_core_without_database_is_one_machine_named_after_it() {
        let mut core = info("odd_core", &["odd"], &[]);
        core.system_name = Some("Odd / Machine".to_owned());
        let rows = rows_of(vec![core], false, "");
        assert_eq!(rows[0].machines.len(), 1);
        assert_eq!(rows[0].machines[0].database, None);
        assert_eq!(rows[0].machines[0].name, "Odd - Machine");
    }

    #[test]
    fn drafts_take_the_probe_extensions_and_distinct_ids() {
        let core = info(
            "fic_core",
            &["old"],
            &["Acme - Fictional Console", "Acme - Fictional Console 2"],
        );
        let machines: Vec<Machine> = machines_of(&core, &Catalog::official());
        let report = report(&["NEW", "zip", "cue"]);
        let drafts = drafts_for(
            &core,
            &machines,
            Some(&report),
            &Catalog::official(),
            1_764_028_800,
            "emu-rust 8.0.0",
        );
        assert_eq!(drafts.len(), 2);
        assert_ne!(drafts[0].id, drafts[1].id);
        assert!(drafts.iter().all(|d| d.extensions == ["new"]));
        assert_eq!(drafts[0].cores, ["fic_core"]);
        assert_eq!(drafts[0].origin.core, "fic_core");
        assert!(drafts[0].dats.is_empty());
        // Both drafts pass the validation the library applies when they are written.
        for d in &drafts {
            emu_library::validate_custom(d, &Catalog::official(), true).unwrap();
        }
    }

    #[test]
    fn without_a_probe_the_info_extensions_are_used() {
        let core = info("fic_core", &["abc", "m3u"], &["Acme - Fictional Console"]);
        assert_eq!(extensions_for(&core, None), ["abc"]);
        assert_eq!(extensions_for(&core, Some(&report(&[]))), ["abc"]);
    }
}
