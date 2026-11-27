//! Sample data for the captures of the views of the custom systems (`--screen custom-add`, `custom-sheet`,
//! `custom-settings`, `switch-banner`, `switch-preview`): nothing of the user's data folder is read for them (the
//! definitions come from a folder of this process, removed once read) and nothing is written to it.

use std::path::Path;

use super::tidy::switch::{SwitchAnalysis, SwitchPreview};
use super::tidy::{Stage, exec::LastTidy};
use super::{LibraryUi, Page};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Demo {
    Add,
    Sheet,
    Settings,
    Banner,
    SwitchPreview,
}

fn definition(id: &str, name: &str, database: &str, extensions: &str, core: &str) -> String {
    format!(
        "format = 1\nrender = \"software\"\n[system]\nid = \"{id}\"\nname = \"{name}\"\ndatabase = \"{database}\"\n\
         extensions = [{extensions}]\n[cores]\nlist = [\"{core}\"]\n\
         [origin]\ncore = \"{core}\"\ncreated_by = \"demo\"\ncreated_unix = 1763840000\n"
    )
}

/// One system alone, one with the id and database of an official system (a switch in place is proposed), a
/// broken file, a removed definition and a replaced one.
fn write_samples(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join(".retired"))?;
    let snes = emu_library::system("snes")
        .and_then(|s| s.libretro_name().map(str::to_owned))
        .unwrap_or_default();
    let files = [
        (
            "vectrex.toml",
            definition(
                "vectrex",
                "Vectrex",
                "GCE - Vectrex",
                "\"bin\", \"vec\"",
                "vecx",
            ),
        ),
        (
            "snes.toml",
            definition(
                "snes",
                "Super Nintendo (mes réglages)",
                &snes,
                "\"sfc\", \"smc\", \"xyz\"",
                "bsnes",
            ),
        ),
        // Same id as an official system, another database: the switch is only maybe the same machine.
        (
            "nes.toml",
            definition(
                "nes",
                "Ma NES",
                "Nintendo - Nintendo Entertainment System (variante)",
                "\"nes\"",
                "nestopia",
            ),
        ),
        ("broken.toml", "not = [toml".to_owned()),
        (
            ".retired/odyssey2-1763840000.toml",
            definition(
                "odyssey2",
                "Odyssey²",
                "Magnavox - Odyssey2",
                "\"bin\"",
                "o2em",
            ),
        ),
        (
            ".retired/gb-1763840001.replaced.toml",
            definition(
                "gb",
                "Game Boy",
                "Nintendo - Game Boy",
                "\"gb\"",
                "gambatte",
            ),
        ),
    ];
    for (name, text) in files {
        std::fs::write(dir.join(name), text)?;
    }
    Ok(())
}

/// A new folder under the temporary one, named with the process id and a random part, created exclusively (an
/// existing name is never reused).
fn private_temp_dir() -> Option<std::path::PathBuf> {
    use std::hash::{BuildHasher, Hasher, RandomState};
    for _ in 0..8 {
        let random = RandomState::new().build_hasher().finish();
        let dir = std::env::temp_dir().join(format!(
            "emu-rust-demo-{}-{random:016x}",
            std::process::id()
        ));
        if std::fs::create_dir(&dir).is_ok() {
            return Some(dir);
        }
    }
    None
}

impl LibraryUi {
    /// Opens `demo` with sample data. Capture mode only.
    pub(super) fn open_custom_demo(&mut self, demo: Demo) {
        if matches!(demo, Demo::Add | Demo::Sheet) {
            self.cores.open_add_demo(demo == Demo::Sheet);
            self.set_page(Page::Cores);
            return;
        }
        // `switch-preview` analyses for real when the data folder proposes a switch (as `library-tidy` does).
        if demo == Demo::SwitchPreview && self.real_switch_proposal().is_some() {
            self.tidy.pending_switch = true;
            return;
        }
        let Some(dir) = private_temp_dir() else {
            tracing::warn!("dossier temporaire des exemples non créé");
            return;
        };
        // Nothing of the real data folder is read by the demo: no journal, no database.
        self.dirs.custom_systems = dir.clone();
        self.dirs.tidy_journals = dir.join("journals");
        self.dirs.library_db = dir.join("library.db");
        if let Err(e) = write_samples(&dir) {
            tracing::warn!("exemples de systèmes non écrits : {e}");
        }
        match demo {
            Demo::Settings => {
                self.set_page(Page::Settings);
                self.custom_systems.scroll_here = true;
            }
            Demo::SwitchPreview => {
                self.refresh_custom_systems();
                self.open_switch_preview_demo();
            }
            _ => self.refresh_custom_systems(),
        }
        // Read once: the page does not read the folder again.
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn real_switch_proposal(&self) -> Option<emu_library::SwitchProposal> {
        emu_library::Catalog::load(&self.dirs.custom_systems)
            .pending_switches
            .into_iter()
            .next()
    }

    /// Frame hook of `--screen switch-preview` with a real proposal: the analysis starts (it needs the context).
    pub(super) fn start_first_proposed_switch(&mut self, ctx: &egui::Context) {
        if let Some(proposal) = self.real_switch_proposal() {
            self.start_switch_analysis(ctx, proposal);
        }
    }

    fn open_switch_preview_demo(&mut self) {
        let Some(proposal) = self.custom_systems.view.as_ref().and_then(|v| {
            let all = &v.load.pending_switches;
            all.iter()
                .find(|p| p.custom_id == "snes")
                .or(all.first())
                .cloned()
        }) else {
            return;
        };
        let analysis = SwitchAnalysis {
            proposal,
            plan: None,
            frozen: ["Hack.xyz", "Demo (beta).xyz", "Translation.smc.xyz"]
                .map(str::to_owned)
                .to_vec(),
            ignored: vec!["roms/mine/Notes.txt".to_owned()],
            last: LastTidy::default(),
        };
        let preview = SwitchPreview::new(analysis, &self.config, &self.dirs);
        self.tidy.stage = Stage::Switch(Box::new(preview));
        self.tidy.focus_first = true;
        self.tidy.back_to = Some(Page::Home);
        self.set_page(Page::Tidy);
    }
}
