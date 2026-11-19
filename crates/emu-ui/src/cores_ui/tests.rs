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
    );
    let LaunchCheck::NeedsInstall(core) = check else {
        panic!("{check:?}");
    };
    assert_eq!(core.as_str(), "genesis_plus_gx");
    // Unknown system and extension.
    let check = plan_launch(manager, &prefs, Some("Nowhere"), Some("zzz"));
    assert!(matches!(check, LaunchCheck::NoCore));
    assert!(check.blocking_message().unwrap().contains("Aucun core"));
    // Game Boy: gambatte is installed but altered → refused, never silently replaced by another core.
    let check = plan_launch(manager, &prefs, Some("Nintendo - Game Boy"), Some("gb"));
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
