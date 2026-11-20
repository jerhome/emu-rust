//! Free BIOS (EmuTOS) on the cores screen and in the launch dialog (ADR 0015 §2): state, variant, install in a
//! background job, remove. Never downloads without a click. A file the user put there is only replaced after an
//! explicit confirmation (`ReplacePolicy::BackupAndReplace`, the old file is renamed, never deleted).

use std::path::PathBuf;
use std::sync::Arc;

use egui::{Button, Color32, RichText, Ui};
use emu_assets::bios::{
    BiosError, BiosVariant, FreeBios, FreeBiosManager, FreeBiosState, ReplacePolicy,
    builtin_free_bios,
};
use emu_assets::cores::FirmwareStatus;
use emu_assets::http::{HttpClient, HttpConfig, Progress};
use emu_assets::jobs::{Job, JobEvent};

use super::{DOWNLOAD_TIMEOUT, ERROR, GOOD, WARNING, text};
use crate::config::DataDirs;

/// Opens the free BIOS manager of the system directory (no network). Shared with the CLI.
pub fn open_free_bios_manager(dirs: &DataDirs) -> Result<Arc<FreeBiosManager>, String> {
    let http = HttpClient::new(HttpConfig {
        timeout: DOWNLOAD_TIMEOUT,
        ..HttpConfig::default()
    })
    .map_err(|e| format!("client HTTP : {e}"))?;
    FreeBiosManager::open(&dirs.system, http)
        .map(Arc::new)
        .map_err(|e| format!("BIOS libres : {e}"))
}

struct Install {
    bios: usize,
    variant: String,
    job: Job<Result<PathBuf, BiosError>>,
    progress: Option<Progress>,
}

/// Outcome of a finished install, for the library (launch, saved variant).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BiosDone {
    pub bios: String,
    pub variant: String,
    pub ok: bool,
    pub error: Option<String>,
}

pub(crate) struct BiosUi {
    manager: Option<Arc<FreeBiosManager>>,
    catalogue: Vec<FreeBios>,
    states: Vec<FreeBiosState>,
    /// Selected variant index per catalogue entry.
    selected: Vec<usize>,
    install: Option<Install>,
    confirm_replace: Option<usize>,
    messages: Vec<Option<(String, Color32)>>,
    done: Vec<BiosDone>,
    dirty: bool,
}

impl BiosUi {
    /// `preferred`: variant id from `[computer] emutos_variant` or the system language.
    pub(crate) fn new(manager: Option<Arc<FreeBiosManager>>, preferred: Option<&str>) -> Self {
        let catalogue = builtin_free_bios();
        let locale = sys_locale::get_locale();
        let selected = catalogue
            .iter()
            .map(|b| {
                let wanted = preferred
                    .and_then(|id| b.variant(id))
                    .or_else(|| b.preferred_variant(locale.as_deref()));
                wanted
                    .and_then(|v| b.variants.iter().position(|x| x.id == v.id))
                    .unwrap_or(0)
            })
            .collect();
        let n = catalogue.len();
        Self {
            manager,
            catalogue,
            states: vec![FreeBiosState::Missing; n],
            selected,
            install: None,
            confirm_replace: None,
            messages: vec![None; n],
            done: Vec::new(),
            dirty: true,
        }
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn refresh(&mut self) {
        if let Some(m) = &self.manager {
            self.states = self.catalogue.iter().map(|b| m.state(b)).collect();
        }
        self.dirty = false;
    }

    pub(crate) fn busy(&self) -> bool {
        self.install.is_some()
    }

    pub(crate) fn progress(&self) -> Option<Progress> {
        self.install.as_ref().and_then(|i| i.progress)
    }

    pub(crate) fn take_done(&mut self) -> Vec<BiosDone> {
        std::mem::take(&mut self.done)
    }

    /// Free BIOS that would satisfy a required BIOS file missing for a core of `system` (database name).
    pub(crate) fn for_missing(&self, system: &str, missing: &[FirmwareStatus]) -> Option<usize> {
        self.catalogue
            .iter()
            .position(|b| b.system == system && missing.iter().any(|f| f.spec.path == b.install_as))
    }

    pub(crate) fn bios(&self, i: usize) -> Option<&FreeBios> {
        self.catalogue.get(i)
    }

    fn variant(&self, i: usize) -> Option<&BiosVariant> {
        self.catalogue.get(i)?.variants.get(*self.selected.get(i)?)
    }

    /// Starts the download (worker thread); one install at a time.
    pub(crate) fn start_install(&mut self, i: usize, policy: ReplacePolicy, ctx: &egui::Context) {
        if self.install.is_some() {
            return;
        }
        let (Some(manager), Some(bios), Some(variant)) = (
            self.manager.clone(),
            self.catalogue.get(i).cloned(),
            self.variant(i).map(|v| v.id.clone()),
        ) else {
            return;
        };
        self.messages[i] = None;
        let ctx = ctx.clone();
        let target = variant.clone();
        let job = Job::spawn(
            &format!("bios-{}", bios.id),
            Arc::new(move || ctx.request_repaint()),
            move |progress, cancel| manager.install(&bios, &target, policy, progress, cancel),
        );
        self.install = Some(Install {
            bios: i,
            variant,
            job,
            progress: None,
        });
    }

    pub(crate) fn cancel(&mut self) {
        if let Some(i) = &self.install {
            i.job.cancel();
        }
    }

    pub(crate) fn poll(&mut self) {
        let Some(install) = &mut self.install else {
            return;
        };
        let mut result = None;
        for ev in install.job.poll() {
            match ev {
                JobEvent::Progress(p) => install.progress = Some(p),
                JobEvent::Done(r) => result = Some(r),
            }
        }
        if result.is_none() && install.job.is_finished() {
            result = Some(Err(BiosError::Archive("tâche interrompue".into())));
        }
        let Some(result) = result else {
            return;
        };
        let Some(install) = self.install.take() else {
            return;
        };
        let i = install.bios;
        let id = self.catalogue[i].id.clone();
        let error = result.as_ref().err().map(text::bios_error);
        self.messages[i] = Some(match &error {
            None => (text::BIOS_INSTALLED_OK.to_owned(), GOOD),
            Some(e) => (e.clone(), ERROR),
        });
        match &result {
            Ok(path) => tracing::info!("BIOS libre installé : {}", path.display()),
            Err(e) => tracing::warn!("installation du BIOS libre {id} : {e}"),
        }
        self.done.push(BiosDone {
            bios: id,
            variant: install.variant,
            ok: result.is_ok(),
            error,
        });
        self.dirty = true;
    }

    /// `< variante >` stepper (Left / Right, Enter); shared with the launch dialog.
    pub(crate) fn variant_picker(
        &mut self,
        ui: &mut Ui,
        i: usize,
        enabled: bool,
    ) -> egui::Response {
        let label = self
            .variant(i)
            .map_or_else(String::new, |v| v.label.clone());
        let resp = ui.add_enabled(enabled, Button::new(format!("<  {label}  >")));
        let delta = crate::ui::stepper(ui, &resp);
        let n = self.catalogue.get(i).map_or(0, |b| b.variants.len());
        if delta != 0 && n > 0 {
            let cur = i64::try_from(self.selected[i]).unwrap_or(0);
            let next = (cur + i64::from(delta)).rem_euclid(i64::try_from(n).unwrap_or(1));
            self.selected[i] = usize::try_from(next).unwrap_or(0);
        }
        resp
    }

    /// Section of the cores screen.
    pub(crate) fn show(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        self.poll();
        if self.dirty {
            self.refresh();
        }
        if self.catalogue.is_empty() || self.manager.is_none() {
            return;
        }
        ui.label(RichText::new(text::BIOS_SECTION).strong());
        for i in 0..self.catalogue.len() {
            self.row(ui, ctx, i);
        }
        ui.label(RichText::new(text::BIOS_NOTE).small().weak());
    }

    fn row(&mut self, ui: &mut Ui, ctx: &egui::Context, i: usize) {
        let bios = self.catalogue[i].clone();
        let state = self.states[i].clone();
        let busy = self.install.is_some();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(text::bios_header(
                &bios.name,
                &bios.version,
                &bios.license,
                &bios.system,
                &bios.install_as,
            ));
            let (status, color) = match &state {
                FreeBiosState::Missing => (text::BIOS_MISSING.to_owned(), Color32::GRAY),
                FreeBiosState::Installed { variant } => (
                    text::bios_installed(
                        &bios
                            .variant(variant)
                            .map_or_else(|| variant.clone(), |v| bios.label(v)),
                    ),
                    GOOD,
                ),
                FreeBiosState::UserFile { md5 } => {
                    match bios.variants.iter().find(|v| v.md5 == *md5) {
                        // A free image copied by hand: fine for the cores, just not managed here.
                        Some(v) => (text::bios_user_copy(&bios.label(v)), GOOD),
                        None => (text::bios_user_file(&bios.install_as), WARNING),
                    }
                }
                FreeBiosState::Modified => (text::bios_modified(&bios.install_as), WARNING),
                FreeBiosState::Unreadable(e) => (text::bios_unreadable(e), ERROR),
            };
            ui.colored_label(color, status);
            ui.horizontal_wrapped(|ui| {
                ui.label(text::BIOS_VARIANT);
                self.variant_picker(ui, i, !busy);
            });
            if let Some(install) = self.install.as_ref().filter(|x| x.bios == i) {
                let progress = install.progress;
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(text::BIOS_INSTALLING);
                    if let Some(p) = progress {
                        ui.label(text::progress(p.done, p.total));
                    }
                    if ui.button(text::CANCEL).clicked() {
                        self.cancel();
                    }
                });
            } else if self.confirm_replace == Some(i) {
                ui.colored_label(WARNING, text::BIOS_REPLACE_CONFIRM);
                ui.horizontal(|ui| {
                    let no = ui.button(text::CONFIRM_REMOVE_NO);
                    no.request_focus();
                    if no.clicked() {
                        self.confirm_replace = None;
                    }
                    if ui.button(text::BIOS_REPLACE_YES).clicked() {
                        self.confirm_replace = None;
                        self.start_install(i, ReplacePolicy::BackupAndReplace, ctx);
                    }
                });
            } else {
                ui.horizontal_wrapped(|ui| {
                    let user_file = is_user_owned(&state);
                    let label = match state {
                        FreeBiosState::Missing => text::BIOS_INSTALL,
                        _ if user_file => text::BIOS_REPLACE,
                        _ => text::BIOS_REINSTALL,
                    };
                    if ui.add_enabled(!busy, Button::new(label)).clicked() {
                        if user_file {
                            self.confirm_replace = Some(i);
                        } else {
                            self.start_install(i, ReplacePolicy::Refuse, ctx);
                        }
                    }
                    let removable = matches!(state, FreeBiosState::Installed { .. });
                    if ui
                        .add_enabled(!busy && removable, Button::new(text::BIOS_REMOVE))
                        .clicked()
                    {
                        self.remove(i);
                    }
                });
            }
            if let Some((msg, color)) = &self.messages[i] {
                ui.colored_label(*color, msg);
            }
        });
    }

    /// Quick (one file + the manifest): not worth a job. Disabled while an install holds the manager's lock.
    fn remove(&mut self, i: usize) {
        let Some(m) = &self.manager else {
            return;
        };
        self.messages[i] = Some(match m.remove(&self.catalogue[i]) {
            Ok(true) => (text::BIOS_REMOVED.to_owned(), GOOD),
            Ok(false) => (text::BIOS_NOT_REMOVED.to_owned(), WARNING),
            Err(e) => (text::bios_error(&e), ERROR),
        });
        self.dirty = true;
    }
}

/// A file the user may care about: replacing it needs confirmation and a backup (never `Refuse`-then-overwrite).
fn is_user_owned(state: &FreeBiosState) -> bool {
    matches!(
        state,
        FreeBiosState::UserFile { .. } | FreeBiosState::Modified | FreeBiosState::Unreadable(_)
    )
}

#[cfg(test)]
mod tests {
    use emu_assets::cores::{FirmwareSpec, FirmwareState};

    use super::*;

    fn missing(path: &str) -> FirmwareStatus {
        FirmwareStatus {
            spec: FirmwareSpec {
                path: path.into(),
                description: String::new(),
                optional: false,
                md5: None,
            },
            state: FirmwareState::Missing,
        }
    }

    #[test]
    fn modified_file_is_treated_as_the_users() {
        assert!(is_user_owned(&FreeBiosState::Modified));
        assert!(is_user_owned(&FreeBiosState::UserFile { md5: [0; 16] }));
        assert!(!is_user_owned(&FreeBiosState::Missing));
        assert!(!is_user_owned(&FreeBiosState::Installed {
            variant: "x".into()
        }));
    }

    #[test]
    fn emutos_is_offered_for_a_missing_tos_only() {
        let ui = BiosUi::new(None, Some("192de"));
        let i = ui.for_missing("Atari - ST", &[missing("tos.img")]).unwrap();
        assert_eq!(ui.bios(i).unwrap().id, "emutos");
        assert_eq!(ui.variant(i).unwrap().id, "192de");
        assert!(
            ui.for_missing("Atari - ST", &[missing("bios.rom")])
                .is_none()
        );
        assert!(
            ui.for_missing("Sega - Mega Drive - Genesis", &[missing("tos.img")])
                .is_none()
        );
        assert!(!ui.busy());
    }

    #[test]
    fn unknown_preferred_variant_falls_back_to_the_catalogue() {
        let ui = BiosUi::new(None, Some("nope"));
        let i = ui.for_missing("Atari - ST", &[missing("tos.img")]).unwrap();
        assert!(ui.variant(i).is_some());
    }
}
