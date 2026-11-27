//! « Fichier d'origine » banner of the selected game (design v7.5 §8, ADR 0023 §5): the core that would run it
//! needs, or works better with, a file the user must provide. State per file (absent, unexpected md5, not
//! verified) and benefit come from `emu_assets::cores::firmware_advice` (`cores.toml`, md5 of `firmware.toml`) or, for a custom system,
//! `firmware_advice_from_info` (the `.info` of the core, minus the files the user ignored).
//! Advice only: nothing is downloaded (ADR 0005), the button opens the existing BIOS import. Computed once per
//! selected game (a directory read), never per frame. The only notice of the library: nothing is announced « once
//! per core and file ».

use emu_assets::cores::{
    AdviceFile, AdviceNeed, CoreChoice, FirmwareAdvice, FirmwareState, KnownFirmware,
    firmware_advice, firmware_advice_from_info,
};
use emu_library::Game;

use super::{LibraryUi, cd32x, grid, text};
use crate::cores_ui;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Banner {
    /// Headline: what the core needs and that emu-rust never downloads it.
    pub text: String,
    /// One line per file: name, state, benefit.
    pub lines: Vec<String>,
    /// At least one listed file is required (the launch will stop on the firmware dialog).
    pub required: bool,
    /// First file to act on: hint of the import dialog.
    pub wanted: String,
}

fn actionable(f: &AdviceFile) -> bool {
    f.need == AdviceNeed::Required || f.state != FirmwareState::Unverified
}

/// `None` when no listed file needs an action (only « non vérifié » files that merely improve the core).
pub(crate) fn banner(advice: &FirmwareAdvice) -> Option<Banner> {
    let to_act: Vec<&AdviceFile> = advice.files.iter().filter(|f| actionable(f)).collect();
    let first = to_act.first()?;
    let required = to_act.iter().any(|f| f.need == AdviceNeed::Required);
    let names: Vec<String> = to_act.iter().map(|f| f.name.clone()).collect();
    Some(Banner {
        text: text::firmware_advice(&names, required),
        lines: advice
            .files
            .iter()
            .map(|f| text::firmware_advice_line(&f.name, state_label(&f.state), &f.benefit))
            .collect(),
        required,
        wanted: first.name.clone(),
    })
}

fn state_label(state: &FirmwareState) -> &'static str {
    match state {
        FirmwareState::Missing | FirmwareState::CoreFallback { .. } => text::FIRMWARE_STATE_MISSING,
        FirmwareState::WrongHash { .. } => text::FIRMWARE_STATE_WRONG,
        FirmwareState::Unreadable(_) => text::FIRMWARE_STATE_UNREADABLE,
        FirmwareState::Unverified => text::FIRMWARE_STATE_UNVERIFIED,
        FirmwareState::Ok | FirmwareState::Free { .. } | FirmwareState::Builtin { .. } => {
            text::FIRMWARE_STATE_OK
        }
    }
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

    /// Something may have changed on disk (import, core installed, preferred core, CD 32X mark): read again.
    pub(super) fn forget_firmware_banner(&mut self) {
        self.advice = None;
    }

    fn compute_banner(&self, game: &Game) -> Option<Banner> {
        let manager = self.cores.manager()?;
        let prefs = cores_ui::preferences(&self.config.cores);
        let database = emu_library::system(&game.system).map(|s| s.core_key().to_owned())?;
        let installed: Vec<_> = manager.installed().into_iter().map(|c| c.id).collect();
        let infos = manager.infos();
        let ext = grid::launch_extension(game);
        let explicit = self.explicit_core(&game.system);
        let needs = cores_ui::GameNeeds {
            needs_32x: cd32x::effective_needs_32x(game, &self.config.library.cd32x),
            explicit: explicit.as_ref(),
        };
        let core = match cores_ui::choose_game_core(
            Some(&database),
            ext.as_deref(),
            &installed,
            &infos,
            &prefs,
            needs,
        ) {
            CoreChoice::Installed(id) | CoreChoice::Install(id) => id,
            CoreChoice::Unknown => return None,
        };
        // Region key of a region table (`"usa"`…); a model table (Amiga) never matches it: unknown variant.
        let variant = crate::launch_firmware::region_variant(
            game.region,
            (!game.is_dir).then_some(game.path.as_path()),
        );
        let known = KnownFirmware::builtin();
        let system = emu_library::system(&game.system)?;
        let advice = if system.is_custom() {
            // A core `cores.toml` does not list: its `.info` names the files; the user may have ignored some.
            let info = infos.get(&core)?;
            let ignored = emu_library::catalog()
                .custom(&system.id)
                .map_or(&[][..], |c| c.ignored_firmware.as_slice());
            firmware_advice_from_info(&core, info, ignored, &self.dirs.system, &known)
        } else {
            firmware_advice(
                &prefs,
                &core,
                &database,
                variant,
                infos.get(&core),
                &self.dirs.system,
                &known,
            )
        };
        match advice {
            Ok(advice) => banner(&advice?),
            Err(e) => {
                tracing::warn!("fichiers d'origine de {core} : {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emu_assets::cores::CoreId;

    fn advice(files: &[(&str, AdviceNeed, FirmwareState)]) -> FirmwareAdvice {
        FirmwareAdvice {
            core: CoreId::new("core").unwrap(),
            files: files
                .iter()
                .map(|(name, need, state)| AdviceFile {
                    name: (*name).into(),
                    need: *need,
                    benefit: "écran de démarrage".into(),
                    state: state.clone(),
                })
                .collect(),
        }
    }

    #[test]
    fn optional_file_says_better() {
        let b = banner(&advice(&[(
            "gba_bios.bin",
            AdviceNeed::Improves,
            FirmwareState::Missing,
        )]))
        .unwrap();
        assert!(!b.required);
        assert_eq!(b.wanted, "gba_bios.bin");
        assert!(b.text.contains("fonctionne mieux avec"), "{}", b.text);
        assert!(b.text.contains("gba_bios.bin"));
        assert!(b.text.contains("ne le télécharge pas"));
        assert_eq!(b.lines.len(), 1);
        assert!(b.lines[0].contains("absent") && b.lines[0].contains("écran de démarrage"));
    }

    #[test]
    fn required_file_says_needs() {
        let b = banner(&advice(&[
            (
                "bios_CD_E.bin",
                AdviceNeed::Required,
                FirmwareState::Missing,
            ),
            (
                "bios_CD_U.bin",
                AdviceNeed::Required,
                FirmwareState::Missing,
            ),
        ]))
        .unwrap();
        assert!(b.required);
        assert!(b.text.contains("exige"), "{}", b.text);
        assert!(b.text.contains("l'un de ces fichiers"), "{}", b.text);
        assert!(b.text.contains("bios_CD_E.bin, bios_CD_U.bin"));
    }

    #[test]
    fn wrong_md5_and_unverified_files_are_told_apart() {
        let b = banner(&advice(&[
            ("a.bin", AdviceNeed::Improves, FirmwareState::Unverified),
            (
                "b.bin",
                AdviceNeed::Improves,
                FirmwareState::WrongHash { md5: [0; 16] },
            ),
        ]))
        .unwrap();
        assert_eq!(b.wanted, "b.bin");
        assert!(!b.text.contains("a.bin"), "{}", b.text);
        assert!(b.lines[0].contains("non vérifié"), "{}", b.lines[0]);
        assert!(b.lines[1].contains("md5 inattendu"), "{}", b.lines[1]);
        // Only unverified files that improve the core: nothing to ask.
        assert_eq!(
            banner(&advice(&[(
                "a.bin",
                AdviceNeed::Improves,
                FirmwareState::Unverified
            )])),
            None
        );
    }
}
