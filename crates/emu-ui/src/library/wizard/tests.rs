//! Wizard logic without a window: start rule, steps, proposals, `wizard_done` written, relaunch.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, RawInput};
use emu_assets::cores::{CoreId, CorePreferences};

use super::*;
use crate::config::DataDirs;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Harness {
    tmp: tempfile::TempDir,
    ctx: egui::Context,
    ui: LibraryUi,
}

/// Empty data root, nothing scanned; not a capture, so the config is really written.
fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = DataDirs::under(tmp.path());
    dirs.create_all().unwrap();
    let mut config = Config::default();
    config.library.scan_on_start = false;
    let ui = LibraryUi::open(dirs, config, false);
    let ctx = egui::Context::default();
    crate::ui::apply_style(&ctx);
    Harness { tmp, ctx, ui }
}

impl Harness {
    fn frame(&mut self, events: Vec<Event>) {
        let raw = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            focused: true,
            events,
            ..RawInput::default()
        };
        let ui = &mut self.ui;
        let mut o = self.ctx.run_ui(raw, |root| {
            ui.show(root);
        });
        o.textures_delta.clear();
    }

    fn key(&mut self, key: Key) {
        let ev = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        self.frame(vec![ev(true)]);
        self.frame(vec![ev(false)]);
    }

    fn saved_config(&self) -> String {
        std::fs::read_to_string(self.tmp.path().join("config.toml")).unwrap_or_default()
    }
}

#[test]
fn starts_only_on_a_fresh_install() {
    let mut c = Config::default();
    assert!(should_start(&c));
    c.library.roots.push(PathBuf::from("/roms"));
    assert!(
        !should_start(&c),
        "existing users with a folder never see it"
    );
    let mut c = Config::default();
    c.ui.wizard_done = true;
    assert!(!should_start(&c));
}

#[test]
fn steps_go_forward_and_back() {
    let mut w = Wizard::new();
    assert_eq!(w.step(), Step::Welcome);
    assert!(!w.back(), "nothing before the first step");
    assert_eq!(w.forward_label(), text::START);
    assert!(!w.forward());
    assert_eq!(w.step(), Step::Roms);
    assert_eq!(w.forward_label(), text::SKIP, "no folder yet: « Passer »");
    w.folder = Some(PathBuf::from("/roms"));
    assert_eq!(w.forward_label(), text::NEXT);
    assert!(!w.forward());
    assert_eq!(w.step(), Step::Cores);
    assert!(w.back());
    assert_eq!(w.step(), Step::Roms);
    assert!(!w.forward());
    assert!(!w.forward());
    assert_eq!(w.step(), Step::Controls);
    assert_eq!(w.forward_label(), text::FINISH);
    assert!(w.forward(), "past the last step: over");
    assert_eq!(
        Step::ALL.map(Step::number),
        [1, 2, 3, 4],
        "« Étape n sur 4 »"
    );
}

#[test]
fn help_anchors_exist_in_the_manual() {
    for step in Step::ALL {
        assert!(
            crate::help::manual().find(step.help_anchor()).is_some(),
            "{step:?}: {}",
            step.help_anchor()
        );
    }
}

fn first_core(prefs: &CorePreferences, system: &str) -> CoreId {
    let db = emu_library::system(system).unwrap().core_key();
    prefs.for_system(db)[0].clone()
}

#[test]
fn proposals_offer_the_missing_preferred_core_of_each_system() {
    let prefs = CorePreferences::builtin();
    let nes_core = first_core(&prefs, "nes");
    let installed: BTreeSet<CoreId> = [nes_core.clone()].into();
    let p = propose_cores(
        ["gb", "nes", "gbc", "gb", "unknown-system"],
        &prefs,
        &installed,
    );
    assert_eq!(p.len(), 3, "one per known system");
    let sorted: Vec<&str> = p.iter().map(|p| p.system_name.as_str()).collect();
    let mut expected = sorted.clone();
    expected.sort_unstable();
    assert_eq!(sorted, expected, "sorted by name");
    let nes = p.iter().find(|p| p.system_id == "nes").unwrap();
    assert_eq!(nes.installed, Some(nes_core), "an installed core covers it");
    assert_eq!(nes.core, None);
    assert!(!nes.selected);
    let gb = p.iter().find(|p| p.system_id == "gb").unwrap();
    assert_eq!(gb.core, Some(first_core(&prefs, "gb")));
    assert!(gb.selected, "missing cores are ticked by default");
    let wanted: BTreeSet<CoreId> = [first_core(&prefs, "gb"), first_core(&prefs, "gbc")].into();
    assert_eq!(
        selected_cores(&p),
        wanted,
        "a shared core is installed once"
    );
}

#[test]
fn a_system_without_a_known_core_offers_nothing() {
    let p = propose_cores(["megadrive"], &CorePreferences::default(), &BTreeSet::new());
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].core, None);
    assert!(!p[0].selected);
    assert!(selected_cores(&p).is_empty());
}

#[test]
fn finishing_writes_wizard_done_and_it_can_be_relaunched() {
    let mut h = harness();
    h.ui.open_wizard();
    h.frame(Vec::new());
    assert_eq!(h.ui.wizard_step(), Some(Step::Welcome));
    for _ in 0..3 {
        h.ui.wizard_nav(WizardNav::Forward);
        h.frame(Vec::new());
    }
    assert_eq!(h.ui.wizard_step(), Some(Step::Controls));
    assert!(!h.ui.config.ui.wizard_done);
    h.ui.wizard_nav(WizardNav::Forward);
    assert_eq!(h.ui.wizard_step(), None);
    assert!(h.ui.config.ui.wizard_done);
    assert!(h.saved_config().contains("wizard_done = true"));
    assert!(!should_start(&h.ui.config));
    h.frame(Vec::new());
    h.ui.open_wizard();
    assert_eq!(
        h.ui.wizard_step(),
        Some(Step::Welcome),
        "relaunch from Réglages"
    );
}

#[test]
fn leaving_early_also_counts_as_done() {
    let mut h = harness();
    h.ui.open_wizard();
    h.ui.wizard_nav(WizardNav::Forward);
    h.ui.wizard_nav(WizardNav::Quit);
    assert_eq!(h.ui.wizard_step(), None);
    assert!(h.saved_config().contains("wizard_done = true"));
}

#[test]
fn escape_goes_back_one_step_and_help_returns_to_the_wizard() {
    let mut h = harness();
    h.ui.open_wizard();
    h.frame(Vec::new());
    h.ui.wizard_nav(WizardNav::Forward);
    h.ui.wizard_nav(WizardNav::Forward);
    h.frame(Vec::new());
    h.key(Key::Escape);
    assert_eq!(h.ui.wizard_step(), Some(Step::Roms));
    h.key(Key::Escape);
    assert_eq!(h.ui.wizard_step(), Some(Step::Welcome));
    h.key(Key::Escape);
    assert_eq!(h.ui.wizard_step(), Some(Step::Welcome));
    h.ui.open_help(Some(Step::Welcome.help_anchor()));
    h.frame(Vec::new());
    h.key(Key::Escape);
    assert_eq!(h.ui.page, Page::Home);
    assert_eq!(
        h.ui.wizard_step(),
        Some(Step::Welcome),
        "resumed after the manual"
    );
}

#[test]
fn chosen_folder_is_saved_scanned_and_its_systems_proposed() {
    let mut h = harness();
    h.ui.open_wizard();
    h.ui.wizard_nav(WizardNav::Forward);
    let ctx = h.ctx.clone();
    assert!(!h.ui.wizard_add_folder(h.tmp.path().join("nope"), &ctx));
    assert!(h.ui.config.library.roots.is_empty());
    let roms = repo_root().join("tests/data/roms");
    assert!(h.ui.wizard_add_folder(roms, &ctx));
    assert_eq!(h.ui.config.library.roots.len(), 1);
    assert!(h.saved_config().contains("roots"));
    let deadline = Instant::now() + Duration::from_secs(30);
    while h.ui.scan.is_some() {
        assert!(Instant::now() < deadline, "scan too long");
        std::thread::sleep(Duration::from_millis(20));
        h.frame(Vec::new());
    }
    assert!(!h.ui.games.is_empty());
    h.ui.wizard_nav(WizardNav::Forward);
    h.frame(Vec::new());
    let w = h.ui.wizard.as_ref().unwrap();
    assert_eq!(w.step(), Step::Cores);
    assert!(!w.proposals.is_empty());
    assert!(w.requested.is_empty(), "nothing downloaded without a click");
    // Each step draws without panicking.
    h.ui.wizard_nav(WizardNav::Forward);
    h.frame(Vec::new());
    h.frame(Vec::new());
}
