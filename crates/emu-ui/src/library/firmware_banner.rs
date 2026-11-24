//! « Fichier d'origine » banner of the selected game (design v7.1 point 5): the core that would run it works
//! better with, or needs, a file the user must provide. Advice only: nothing is downloaded (ADR 0005), the
//! button opens the existing BIOS import. Computed once per selected game (a directory read), never per frame.

use emu_assets::cores::{CoreChoice, choose_core};
use emu_assets::{FirmwareAdvice, firmware_advice};
use emu_library::Game;

use super::{LibraryUi, grid, text};
use crate::cores_ui;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Banner {
    pub text: String,
    /// At least one missing file is required (the launch will stop on the firmware dialog).
    pub required: bool,
    /// First missing file: hint of the import dialog.
    pub wanted: String,
}

/// `None` when every file of the advice is present.
pub(crate) fn banner(advice: &FirmwareAdvice) -> Option<Banner> {
    let missing: Vec<_> = advice.files.iter().filter(|f| !f.present).collect();
    let first = missing.first()?;
    let required = missing.iter().any(|f| f.required);
    let names: Vec<String> = missing.iter().map(|f| f.name.clone()).collect();
    Some(Banner {
        text: text::firmware_advice(&names, required),
        required,
        wanted: first.name.clone(),
    })
}

impl LibraryUi {
    /// Banner of the selected game, cached per game until [`Self::forget_firmware_banner`].
    pub(super) fn firmware_banner(&mut self) -> Option<Banner> {
        let game = self.selected_game()?.clone();
        if let Some((id, cached)) = &self.advice
            && *id == game.id
        {
            return cached.clone();
        }
        let computed = self.compute_banner(&game);
        self.advice = Some((game.id, computed.clone()));
        computed
    }

    /// Something may have changed on disk (import, core installed, preferred core): read again.
    pub(super) fn forget_firmware_banner(&mut self) {
        self.advice = None;
    }

    fn compute_banner(&self, game: &Game) -> Option<Banner> {
        let manager = self.cores.manager()?;
        let prefs = cores_ui::preferences(&self.config.cores);
        let database = emu_library::system(&game.system).map(|s| s.database.as_str());
        let installed: Vec<_> = manager.installed().into_iter().map(|c| c.id).collect();
        let ext = grid::launch_extension(game);
        let core = match choose_core(
            database,
            ext.as_deref(),
            &installed,
            &manager.infos(),
            &prefs,
        ) {
            CoreChoice::Installed(id) | CoreChoice::Install(id) => id,
            CoreChoice::Unknown => return None,
        };
        let advice = firmware_advice(core.as_str(), &game.system, &self.dirs.system)?;
        banner(&advice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_assets::AdviceFile;

    fn advice(files: &[(&str, bool, bool)]) -> FirmwareAdvice {
        FirmwareAdvice {
            core_id: "core".into(),
            files: files
                .iter()
                .map(|&(name, required, present)| AdviceFile {
                    name: name.into(),
                    required,
                    present,
                })
                .collect(),
            note: String::new(),
        }
    }

    #[test]
    fn optional_file_says_better() {
        let b = banner(&advice(&[("gba_bios.bin", false, false)])).unwrap();
        assert!(!b.required);
        assert_eq!(b.wanted, "gba_bios.bin");
        assert!(b.text.contains("fonctionne mieux avec"), "{}", b.text);
        assert!(b.text.contains("gba_bios.bin"));
        assert!(b.text.contains("ne le télécharge pas"));
    }

    #[test]
    fn required_file_says_needs() {
        let b = banner(&advice(&[
            ("bios_CD_E.bin", true, false),
            ("bios_CD_U.bin", true, false),
        ]))
        .unwrap();
        assert!(b.required);
        assert!(b.text.contains("exige"), "{}", b.text);
        assert!(b.text.contains("l'un de ces fichiers"), "{}", b.text);
        assert!(b.text.contains("bios_CD_E.bin, bios_CD_U.bin"));
    }

    #[test]
    fn present_files_are_not_listed() {
        let b = banner(&advice(&[("a.bin", false, true), ("b.bin", false, false)])).unwrap();
        assert_eq!(b.wanted, "b.bin");
        assert!(!b.text.contains("a.bin"));
        assert_eq!(banner(&advice(&[("a.bin", true, true)])), None);
    }
}
