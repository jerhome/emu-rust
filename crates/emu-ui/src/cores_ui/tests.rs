//! Screen and launch tests on temporary data directories (no network).

use std::fs;

use emu_assets::cores::{CoreChoice, CoreId, Platform};

use super::*;

fn id(s: &str) -> CoreId {
    CoreId::new(s).unwrap()
}

/// A data root with one manual core, one altered managed core (wrong SHA-256) and a `.info` with a required
/// BIOS. `None` on platforms without a buildbot path.
fn fixture() -> Option<(tempfile::TempDir, DataDirs)> {
    let platform = Platform::current()?;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = DataDirs::under(tmp.path());
    dirs.create_all().unwrap();
    let ext = platform.library_extension();
    fs::write(dirs.cores.join(format!("handmade_libretro.{ext}")), b"x").unwrap();
    fs::write(
        dirs.cores.join(format!("gambatte_libretro.{ext}")),
        b"changed",
    )
    .unwrap();
    fs::write(
        dirs.cores.join("manifest.toml"),
        format!(
            "version = 1\n[cores.gambatte]\nfile = \"gambatte_libretro.{ext}\"\nurl = \"https://buildbot.libretro.com/x\"\n\
             date = \"2026-11-16\"\ncrc32 = \"e09352e9\"\nsha256 = \"{}\"\nsize = 7\ninstalled_unix = 1\npinned = false\n",
            "0".repeat(64)
        ),
    )
    .unwrap();
    let info = dirs.cores.join("info");
    fs::create_dir_all(&info).unwrap();
    fs::write(
        info.join("handmade_libretro.info"),
        "display_name = \"Handmade\"\ndatabase = \"Nintendo - Game Boy\"\nsupported_extensions = \"gb\"\n\
         firmware_count = 2\nfirmware0_desc = \"req\"\nfirmware0_path = \"req.bin\"\nfirmware0_opt = \"false\"\n\
         firmware1_desc = \"opt\"\nfirmware1_path = \"opt.bin\"\nfirmware1_opt = \"true\"\n",
    )
    .unwrap();
    Some((tmp, dirs))
}

fn ui_for(dirs: &DataDirs) -> CoresUi {
    CoresUi::open(dirs, &Config::default())
}

#[test]
fn altered_core_is_never_loaded() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    let ui = ui_for(&dirs);
    let manager = ui.manager().unwrap();
    let check = prepare_launch(manager, &id("gambatte"));
    let LaunchCheck::Failed(CoreError::Tampered { .. }) = &check else {
        panic!("{check:?}");
    };
    assert!(check.blocking_message().unwrap().contains("Réinstaller"));
    // Not held in use: the user may reinstall it.
    assert!(manager.remove(&id("gambatte")).is_ok());
}

#[test]
fn required_firmware_asks_before_launching_and_holds_the_core() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    let ui = ui_for(&dirs);
    let manager = ui.manager().unwrap();
    let check = prepare_launch(manager, &id("handmade"));
    let LaunchCheck::FirmwareMissing { missing, .. } = &check else {
        panic!("{check:?}");
    };
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].spec.path, "req.bin");
    assert!(matches!(
        manager.remove(&id("handmade")),
        Err(CoreError::InUse(_))
    ));
    release(manager);
    assert!(manager.remove(&id("handmade")).is_ok());
}

#[test]
fn plan_launch_picks_installed_proposes_or_gives_up() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    let ui = ui_for(&dirs);
    let manager = ui.manager().unwrap();
    let prefs = preferences(&CoresConfig::default());
    // Mega Drive: nothing installed → install the recommended core.
    let check = plan_launch(
        manager,
        &prefs,
        Some("Sega - Mega Drive - Genesis"),
        Some("md"),
        GameNeeds::default(),
    );
    let LaunchCheck::NeedsInstall(core) = check else {
        panic!("{check:?}");
    };
    assert_eq!(core.as_str(), "genesis_plus_gx");
    // Unknown system and extension.
    let check = plan_launch(
        manager,
        &prefs,
        Some("Nowhere"),
        Some("zzz"),
        GameNeeds::default(),
    );
    assert!(matches!(check, LaunchCheck::NoCore));
    assert!(check.blocking_message().unwrap().contains("Aucun core"));
    // Game Boy: gambatte is installed but altered → refused, never silently replaced by another core.
    let check = plan_launch(
        manager,
        &prefs,
        Some("Nintendo - Game Boy"),
        Some("gb"),
        GameNeeds::default(),
    );
    assert!(matches!(
        check,
        LaunchCheck::Failed(CoreError::Tampered { .. })
    ));
    assert_eq!(
        emu_assets::cores::choose_core(
            Some("Nintendo - Game Boy"),
            None,
            &[id("gambatte")],
            &BTreeMap::new(),
            &prefs
        ),
        CoreChoice::Installed(id("gambatte"))
    );
}

const MEGACD: &str = "Sega - Mega-CD - Sega CD";

fn choose_megacd(installed: &[&str], prefs: &CorePreferences, needs: GameNeeds<'_>) -> CoreChoice {
    let installed: Vec<CoreId> = installed.iter().map(|c| id(c)).collect();
    choose_game_core(
        Some(MEGACD),
        Some("cue"),
        &installed,
        &BTreeMap::new(),
        prefs,
        needs,
    )
}

#[test]
fn disc_32x_gets_a_32x_core_other_megacd_games_unchanged() {
    let prefs = preferences(&CoresConfig::default());
    let cd32x = GameNeeds {
        needs_32x: true,
        explicit: None,
    };
    // picodrive missing: proposed for install even though genesis_plus_gx (Mega CD only) is there.
    assert_eq!(
        choose_megacd(&["genesis_plus_gx"], &prefs, cd32x),
        CoreChoice::Install(id("picodrive"))
    );
    assert_eq!(
        choose_megacd(&["genesis_plus_gx", "picodrive"], &prefs, cd32x),
        CoreChoice::Installed(id("picodrive"))
    );
    // An ordinary Mega CD game keeps the built-in order.
    assert_eq!(
        choose_megacd(
            &["genesis_plus_gx", "picodrive"],
            &prefs,
            GameNeeds::default()
        ),
        CoreChoice::Installed(id("genesis_plus_gx"))
    );
}

#[test]
fn disc_32x_honours_only_a_32x_capable_user_choice() {
    // The user prefers genesis_plus_gx for the Mega CD: overridden (warning in the log) for a 32X disc only.
    let gpgx = id("genesis_plus_gx");
    let mut config = CoresConfig::default();
    config.preferred.insert("megacd".into(), gpgx.to_string());
    let prefs = preferences(&config);
    let needs = GameNeeds {
        needs_32x: true,
        explicit: Some(&gpgx),
    };
    assert_eq!(
        choose_megacd(&["genesis_plus_gx", "picodrive"], &prefs, needs),
        CoreChoice::Installed(id("picodrive"))
    );
    // A capable explicit choice wins over the built-in 32X order.
    let handmade = id("handmade");
    let mut config = CoresConfig::default();
    config.preferred.insert("megacd".into(), "handmade".into());
    config.preferred.insert("32x".into(), "handmade".into());
    let prefs = preferences(&config);
    let needs = GameNeeds {
        needs_32x: true,
        explicit: Some(&handmade),
    };
    assert_eq!(
        choose_megacd(&["handmade", "picodrive"], &prefs, needs),
        CoreChoice::Installed(handmade.clone())
    );
    assert_eq!(
        choose_megacd(&["picodrive"], &prefs, needs),
        CoreChoice::Installed(id("picodrive"))
    );
}

#[test]
fn disc_32x_ignores_a_user_override_of_the_32x_list_without_32x() {
    let gpgx = id("genesis_plus_gx");
    let mut config = CoresConfig::default();
    config.preferred.insert("32x".into(), gpgx.to_string());
    let prefs = preferences(&config);
    let needs = GameNeeds {
        needs_32x: true,
        explicit: None,
    };
    assert_eq!(
        choose_megacd(&["genesis_plus_gx", "picodrive"], &prefs, needs),
        CoreChoice::Installed(id("picodrive"))
    );
    assert_eq!(
        choose_megacd(&["genesis_plus_gx"], &prefs, needs),
        CoreChoice::Install(id("picodrive"))
    );
}

#[test]
fn preferences_convert_library_ids_to_database_names() {
    let mut cfg = CoresConfig::default();
    cfg.preferred.insert("gb".into(), "sameboy".into());
    cfg.preferred.insert("nope".into(), "mgba".into());
    let prefs = preferences(&cfg);
    assert_eq!(
        prefs.for_system("Nintendo - Game Boy")[0].as_str(),
        "sameboy"
    );
}

/// Runs one egui pass of the screen headlessly.
fn pass(ctx: &egui::Context, ui: &mut CoresUi) -> Option<CoresEvent> {
    let mut event = None;
    let mut out = ctx.run_ui(egui::RawInput::default(), |root| {
        event = ui.show(root);
    });
    out.textures_delta.clear();
    event
}

#[test]
fn screen_renders_rows_and_never_blocks() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    let mut cores = ui_for(&dirs);
    let ctx = egui::Context::default();
    assert_eq!(pass(&ctx, &mut cores), None);
    let rows = cores.visible_rows();
    let state = |name: &str| rows.iter().find(|r| r.id.as_str() == name).unwrap().state;
    assert_eq!(state("gambatte"), CoreState::Tampered);
    assert_eq!(state("handmade"), CoreState::Manual);
    assert_eq!(state("snes9x"), CoreState::NotInstalled);
    // The in-use core cannot be changed from the screen.
    cores.set_in_use(Some(id("handmade")));
    assert!(
        cores
            .visible_rows()
            .iter()
            .any(|r| r.id.as_str() == "handmade" && r.in_use)
    );
    // Detail view lists the BIOS (and renders).
    cores.detail = Some(id("handmade"));
    assert_eq!(pass(&ctx, &mut cores), None);
}

#[test]
fn escape_closes_detail_then_leaves() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    let mut cores = ui_for(&dirs);
    cores.detail = Some(id("handmade"));
    let ctx = egui::Context::default();
    let esc = || egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..egui::RawInput::default()
    };
    let run = |cores: &mut CoresUi| {
        let mut event = None;
        let mut out = ctx.run_ui(esc(), |root| event = cores.show(root));
        out.textures_delta.clear();
        event
    };
    assert_eq!(run(&mut cores), None);
    assert!(cores.detail.is_none());
    assert_eq!(run(&mut cores), Some(CoresEvent::Back));
}

#[test]
fn request_install_without_catalog_queues_and_checks_first() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    // An unusable source: the check fails fast without touching the real buildbot.
    let mut config = Config::default();
    config.cores.buildbot_url = Some("http://127.0.0.1:9".into());
    let mut cores = CoresUi::open(&dirs, &config);
    let ctx = egui::Context::default();
    cores.request_install(id("snes9x"), &ctx);
    assert_eq!(cores.queued, vec![id("snes9x")]);
    assert!(cores.refresh.is_some());
    // The UI thread never waits: poll until the failed job is reported, then the queue is dropped.
    for _ in 0..200 {
        let _ = pass(&ctx, &mut cores);
        if cores.refresh.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(cores.refresh.is_none());
    assert!(cores.queued.is_empty());
    assert!(cores.refresh_error.is_some());
    assert!(cores.installs.is_empty());
}

/// Runs frames until no manager operation is in flight.
fn settle(ctx: &egui::Context, cores: &mut CoresUi) {
    for _ in 0..200 {
        let _ = pass(ctx, cores);
        if cores.ops.is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("operation never finished");
}

#[test]
fn remove_and_pin_run_in_jobs() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    let mut cores = ui_for(&dirs);
    let ctx = egui::Context::default();
    cores.apply(RowAction::Remove(id("handmade")), &ctx);
    assert!(
        cores.ops.contains_key(&id("handmade")),
        "job started, not run inline"
    );
    // Showing the screen with the operation in flight renders its state.
    let _ = pass(&ctx, &mut cores);
    settle(&ctx, &mut cores);
    assert!(
        !cores
            .manager()
            .unwrap()
            .installed()
            .iter()
            .any(|c| c.id.as_str() == "handmade")
    );
    assert!(cores.errors.is_empty());
    // A held core refuses the operation: the French message is kept on the row.
    cores.manager().unwrap().set_in_use(Some(id("gambatte")));
    cores.apply(RowAction::Remove(id("gambatte")), &ctx);
    settle(&ctx, &mut cores);
    assert!(cores.errors[&id("gambatte")].contains("utilisation"));
}

#[test]
fn rollback_of_altered_core_offers_reinstall() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    let platform = Platform::current().unwrap();
    fs::write(
        dirs.cores.join(format!(
            "gambatte_libretro.{}.prev",
            platform.library_extension()
        )),
        b"old",
    )
    .unwrap();
    let mut cores = ui_for(&dirs);
    let ctx = egui::Context::default();
    cores.apply(RowAction::Rollback(id("gambatte")), &ctx);
    settle(&ctx, &mut cores);
    assert!(cores.offer_reinstall.contains(&id("gambatte")));
    assert!(cores.errors[&id("gambatte")].contains("Réinstaller"));
    // The row renders with the button; starting an install clears the offer.
    let _ = pass(&ctx, &mut cores);
    cores.apply(RowAction::Install(id("gambatte")), &ctx);
    assert!(cores.offer_reinstall.is_empty());
    for run in cores.installs.values() {
        run.job.cancel();
    }
}

#[test]
fn details_use_the_snapshot_firmware_not_a_per_frame_hash() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    let mut cores = ui_for(&dirs);
    let ctx = egui::Context::default();
    let _ = pass(&ctx, &mut cores);
    cores.apply(RowAction::Details(id("handmade")), &ctx);
    let before: Vec<String> = cores
        .detail_firmware
        .iter()
        .map(|f| format!("{}:{:?}", f.spec.path, f.state))
        .collect();
    assert_eq!(before.len(), 2);
    assert!(before[0].contains("Missing"), "{before:?}");
    // The BIOS appears on disk: frames do not hash it again (refreshed with the snapshot only).
    fs::write(dirs.system.join("req.bin"), b"bios").unwrap();
    for _ in 0..3 {
        let _ = pass(&ctx, &mut cores);
    }
    let after: Vec<String> = cores
        .detail_firmware
        .iter()
        .map(|f| format!("{}:{:?}", f.spec.path, f.state))
        .collect();
    assert_eq!(before, after);
    cores.dirty = true;
    let _ = pass(&ctx, &mut cores);
    assert!(!format!("{:?}", cores.detail_firmware[0].state).contains("Missing"));
}

#[test]
fn escape_belongs_to_the_host_dialog() {
    let Some((_tmp, dirs)) = fixture() else {
        return;
    };
    let mut cores = ui_for(&dirs);
    let ctx = egui::Context::default();
    let esc = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..egui::RawInput::default()
    };
    cores.set_modal_open(true);
    let mut event = None;
    let mut out = ctx.run_ui(esc.clone(), |root| event = cores.show(root));
    out.textures_delta.clear();
    assert_eq!(event, None);
    cores.set_modal_open(false);
    let mut out = ctx.run_ui(esc, |root| event = cores.show(root));
    out.textures_delta.clear();
    assert_eq!(event, Some(CoresEvent::Back));
}

#[test]
fn core_choice_lists_single_core_systems_too() {
    let prefs = emu_assets::cores::CorePreferences::builtin();
    let systems = super::choice_systems(&prefs);
    let three_do = systems
        .iter()
        .find(|(s, _)| s.contains("3DO"))
        .expect("3DO listed");
    assert_eq!(three_do.1, [id("opera")]);
    assert!(systems.iter().any(|(_, c)| c.len() > 1), "choices kept");
}
