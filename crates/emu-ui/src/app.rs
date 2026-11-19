//! winit `ApplicationHandler` driving the emulator: `Pacer` scheduling, shortcuts, SRAM flushing, and the egui
//! overlay (menu open ⇒ emulation paused, keys go to egui instead of the game).

use std::time::{Duration, Instant};

use emu_runtime::{
    AudioConfig, AudioOutput, CoreOption, CoreOptionCategory, Emulator, EmulatorOptions, GlWindow,
    LogLevel, MAX_BURST, PaceDecision, Pacer, STATE_SLOTS, Scaling, SyncMode, core_id_from_path,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, WindowAttributes, WindowId};

use crate::config::{MAX_SCALE, MIN_SCALE};
use crate::egui_layer::EguiLayer;
use crate::hotkeys::{HotkeyAction, SpeedState, step_slot};
use crate::keyboard::KeyMap;
use crate::ui::{
    self, CaptureOutcome, OsdLevel, Overlay, StatsView, UiAction, UiData, apply_capture,
    profile_map, text,
};
use crate::{KeyboardInput, LaunchOptions, ScalingChoice, Screen, SyncChoice, UiError};

const SRAM_FLUSH_INTERVAL: Duration = Duration::from_secs(10);
/// Frames emulated before a `--screen` capture when `--frames` is absent.
const DEFAULT_SCREEN_FRAMES: u64 = 30;
/// egui passes rendered before a `--screen` capture (window auto-sizing needs a couple of passes).
const SCREEN_CAPTURE_PASSES: u32 = 4;
/// The initial window is grown (by whole scale steps) up to this size so the menu fits.
const MIN_WINDOW: (u32, u32) = (640, 480);

// Field order is the drop order: egui painter, window/context (with its renderer), then the emulator
// (ADR 0008). The painter is also destroyed explicitly while its context is alive (`teardown`).
struct App {
    egui: Option<EguiLayer>,
    window: Option<GlWindow>,
    emulator: Emulator,
    pacer: Option<Pacer>,
    keyboard: KeyboardInput,
    options: LaunchOptions,
    audio: AudioOutput,
    error: Option<UiError>,
    core_id: String,
    speed: SpeedState,
    slot: u8,
    next_flush: Instant,
    fullscreen: bool,
    overlay: Overlay,
    /// `created_unix` per slot; refreshed when the menu opens and after a save (reading a slot verifies the file).
    slots: Vec<Option<u64>>,
    core_options: Vec<CoreOption>,
    categories: Vec<CoreOptionCategory>,
    config_dirty: bool,
    fps: FpsMeter,
    /// `--screen` / `--dump-window`: egui passes left before capturing and quitting; emulation is stopped.
    finishing: Option<u32>,
    /// Next repaint wanted by egui or the OSD while the game is not driving redraws.
    ui_repaint_at: Option<Instant>,
}

struct FpsMeter {
    since: Instant,
    frames_at: u64,
    fps: f64,
}

impl FpsMeter {
    fn update(&mut self, now: Instant, frames: u64) {
        let elapsed = now.duration_since(self.since);
        if elapsed >= Duration::from_secs(1) {
            #[allow(clippy::cast_precision_loss)] // frame counts are far below 2^52
            let n = frames.saturating_sub(self.frames_at) as f64;
            self.fps = n / elapsed.as_secs_f64();
            self.since = now;
            self.frames_at = frames;
        }
    }
}

/// Initial scale: the configured one, grown until the window reaches `MIN_WINDOW` (bounded by `MAX_SCALE`).
fn initial_scale(base: (u32, u32), wanted: u32) -> u32 {
    let mut scale = wanted.clamp(MIN_SCALE, MAX_SCALE);
    while scale < MAX_SCALE
        && (base.0.saturating_mul(scale) < MIN_WINDOW.0
            || base.1.saturating_mul(scale) < MIN_WINDOW.1)
    {
        scale += 1;
    }
    scale
}

pub(crate) fn run(mut options: LaunchOptions) -> Result<(), UiError> {
    let core_id = core_id_from_path(&options.core);
    let emulator = Emulator::load(
        &options.core,
        &options.rom,
        EmulatorOptions {
            system_dir: options.dirs.system.clone(),
            save_dir: options.dirs.saves.clone(),
            state_dir: options.dirs.states.clone(),
            assets_dir: options.dirs.assets.clone(),
            core_variables: options.config.core_variables(&core_id),
        },
    )?;
    let audio = if options.audio && options.config.audio.enabled {
        let cfg = AudioConfig {
            latency: Duration::from_millis(u64::from(options.config.audio.latency_ms.max(1))),
            ..AudioConfig::default()
        };
        AudioOutput::open_or_null(emulator.av_info().timing.sample_rate, &cfg)
    } else {
        AudioOutput::null()
    };
    let keymap = options.config.keymap_for(&core_id);
    tracing::info!(
        "{} touches liées (profil de {core_id})",
        keymap.bindings().len()
    );
    if options.dump_window.is_some() || options.screen.is_some() {
        options
            .exit_after_frames
            .get_or_insert(DEFAULT_SCREEN_FRAMES);
    }
    let event_loop = EventLoop::new().map_err(|e| UiError::EventLoop(e.to_string()))?;
    let now = Instant::now();
    let mut overlay = Overlay::default();
    overlay
        .osd
        .info(text::menu_hint(options.config.hotkeys.menu), now);
    let mut app = App {
        egui: None,
        window: None,
        emulator,
        pacer: None,
        keyboard: KeyboardInput::new(keymap),
        options,
        audio,
        error: None,
        core_id,
        speed: SpeedState::default(),
        slot: 0,
        next_flush: now + SRAM_FLUSH_INTERVAL,
        fullscreen: false,
        overlay,
        slots: Vec::new(),
        core_options: Vec::new(),
        categories: Vec::new(),
        config_dirty: false,
        fps: FpsMeter {
            since: now,
            frames_at: 0,
            fps: 0.0,
        },
        finishing: None,
        ui_repaint_at: None,
    };
    let result = event_loop.run_app(&mut app);
    app.teardown();
    app.flush_sram();
    app.save_config();
    let stats = app.audio.stats();
    tracing::info!(
        "audio stats: underruns={} dropped_samples={} stream_failed={}",
        stats.underruns,
        stats.dropped_samples,
        stats.stream_failed
    );
    result.map_err(|e| UiError::EventLoop(e.to_string()))?;
    if let Some(e) = app.error.take() {
        return Err(e);
    }
    if let Some(path) = &app.options.dump_last_frame {
        app.emulator.frame().write_png(path)?;
    }
    Ok(())
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl Into<UiError>) {
        self.error = Some(error.into());
        event_loop.exit();
    }

    /// egui's GL objects go before the context (ADR 0008 drop order).
    fn teardown(&mut self) {
        if let (Some(egui), Some(window)) = (&mut self.egui, &self.window) {
            egui.destroy(window);
        }
        self.egui = None;
        self.window = None;
    }

    fn flush_sram(&mut self) {
        match self.emulator.flush_sram() {
            Ok(true) => tracing::info!("SRAM écrite : {}", self.emulator.sram_path().display()),
            Ok(false) => {}
            Err(e) => tracing::warn!("écriture de la SRAM impossible : {e}"),
        }
    }

    /// Writes `config.toml` if something changed; a locked (invalid) file is never touched.
    fn save_config(&mut self) {
        if !std::mem::take(&mut self.config_dirty) {
            return;
        }
        let now = Instant::now();
        match self.options.config.save(&self.options.dirs.config_file) {
            Ok(()) => {
                tracing::info!("configuration enregistrée");
                self.overlay.osd.info(text::CONFIG_SAVED, now);
            }
            Err(e) => {
                tracing::warn!("configuration non enregistrée : {e}");
                let msg = if self.options.config.is_locked() {
                    text::CONFIG_LOCKED.to_owned()
                } else {
                    text::config_save_failed(&e)
                };
                self.overlay.osd.error(msg, now);
            }
        }
    }

    fn effective_speed(&self) -> SpeedState {
        SpeedState {
            paused: self.speed.paused || self.overlay.is_open() || self.finishing.is_some(),
            fast_forward: self.speed.fast_forward,
        }
    }

    /// Pushes the effective speed to the pacer, the audio device and the core.
    fn apply_speed(&mut self) {
        let now = Instant::now();
        let speed = self.effective_speed();
        if let Some(pacer) = &mut self.pacer {
            pacer.set_speed(
                speed.speed(self.options.config.speed.fast_forward_ratio),
                now,
            );
        }
        self.audio.set_paused(speed.audio_paused());
        self.emulator.set_fast_forwarding(speed.core_fast_forward());
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.window().request_redraw();
        }
    }

    fn refresh_slots(&mut self) {
        self.slots = (0..STATE_SLOTS)
            .map(|s| self.emulator.state_info(s).map(|(h, _)| h.created_unix))
            .collect();
    }

    fn refresh_options(&mut self) {
        self.core_options = self.emulator.options();
        self.categories = self.emulator.option_categories();
    }

    fn open_screen(&mut self, screen: Screen) {
        if self.egui.is_none() {
            return;
        }
        if !self.overlay.is_open() {
            // The game must not keep a key held, nor fast-forward, while the menu has the keyboard.
            self.keyboard.release_all();
            self.speed.fast_forward = false;
            self.flush_sram();
        }
        match screen {
            Screen::Menu => self.refresh_slots(),
            Screen::CoreOptions => self.refresh_options(),
            Screen::Keys => {}
        }
        self.overlay.open(screen);
        self.apply_speed();
        self.request_redraw();
    }

    fn close_overlay(&mut self) {
        self.overlay.close();
        self.save_config();
        self.apply_speed();
        self.request_redraw();
    }

    fn go_back(&mut self) {
        let before = self.overlay.screen();
        self.overlay.back();
        let after = self.overlay.screen();
        if before != after {
            self.save_config();
        }
        match after {
            None => self.close_overlay(),
            Some(Screen::Menu) if before != after => self.refresh_slots(),
            _ => {}
        }
        self.request_redraw();
    }

    fn rebuild_keyboard(&mut self) {
        self.keyboard = KeyboardInput::new(self.options.config.keymap_for(&self.core_id));
    }

    fn finish_capture(&mut self, key: KeyCode) {
        let Some((profile, port, button)) = self.overlay.capture() else {
            return;
        };
        let config = &self.options.config;
        let mut map = profile_map(config, &self.core_id, profile);
        let outcome = apply_capture(&mut map, &config.hotkeys, key, port, button);
        let notice = match outcome {
            CaptureOutcome::Cancelled => text::KEYS_CANCELLED.to_owned(),
            CaptureOutcome::Cleared => text::KEYS_CLEARED.to_owned(),
            CaptureOutcome::Refused(action) => text::keys_refused(key, action),
            CaptureOutcome::Bound {
                moved_from: Some(from),
            } => text::keys_moved(key, from, (port, button)),
            CaptureOutcome::Bound { moved_from: None } => text::keys_bound(key, port, button),
        };
        if matches!(
            outcome,
            CaptureOutcome::Cleared | CaptureOutcome::Bound { .. }
        ) {
            let profile_key = profile.key(&self.core_id).to_owned();
            self.options.config.set_keymap(&profile_key, map);
            self.config_dirty = true;
            self.rebuild_keyboard();
        }
        self.overlay.end_capture(notice);
        self.request_redraw();
    }

    fn save_state(&mut self) {
        let now = Instant::now();
        match self.emulator.save_state(self.slot) {
            Ok(path) => {
                tracing::info!("état {} sauvegardé ({})", self.slot, path.display());
                self.overlay.osd.info(text::state_saved(self.slot), now);
            }
            Err(e) => {
                tracing::warn!("sauvegarde de l'état {} impossible : {e}", self.slot);
                self.overlay
                    .osd
                    .error(text::state_save_failed(self.slot, &e), now);
            }
        }
        if self.overlay.is_open() {
            self.refresh_slots();
        }
    }

    fn load_state(&mut self) -> bool {
        let now = Instant::now();
        match self.emulator.load_state(self.slot) {
            Ok(()) => {
                tracing::info!("état {} chargé", self.slot);
                self.overlay.osd.info(text::state_loaded(self.slot), now);
                true
            }
            Err(e) => {
                tracing::warn!("chargement de l'état {} impossible : {e}", self.slot);
                self.overlay
                    .osd
                    .error(text::state_load_failed(self.slot, &e), now);
                false
            }
        }
    }

    fn set_option(&mut self, key: &str, value: &str) {
        if let Err(e) = self.emulator.set_option(key, value) {
            tracing::warn!("option {key} refusée : {e}");
            self.overlay
                .osd
                .error(text::option_failed(&e), Instant::now());
            return;
        }
        let default = self
            .core_options
            .iter()
            .find(|o| o.key == key)
            .map(|o| o.default.clone());
        let saved = self
            .options
            .config
            .core_options
            .entry(self.core_id.clone())
            .or_default();
        // Only overrides are persisted, so a new core default is picked up later.
        if default.as_deref() == Some(value) {
            saved.remove(key);
        } else {
            saved.insert(key.to_owned(), value.to_owned());
        }
        if saved.is_empty() {
            self.options.config.core_options.remove(&self.core_id);
        }
        self.config_dirty = true;
    }

    fn apply_actions(&mut self, event_loop: &ActiveEventLoop, actions: Vec<UiAction>) {
        for action in actions {
            match action {
                UiAction::Resume => self.close_overlay(),
                UiAction::Reset => {
                    self.emulator.reset();
                    self.overlay.osd.info(text::RESET_DONE, Instant::now());
                    self.close_overlay();
                }
                UiAction::SelectSlot(s) => self.slot = s,
                UiAction::SaveState => self.save_state(),
                UiAction::LoadState => {
                    if self.load_state() {
                        self.close_overlay();
                    }
                }
                UiAction::Open(screen) => self.open_screen(screen),
                UiAction::Back => self.go_back(),
                UiAction::Quit => event_loop.exit(),
                UiAction::SetOption { key, value } => {
                    self.set_option(&key, &value);
                    self.refresh_options();
                }
                UiAction::DefaultOptions => {
                    let changes: Vec<(String, String)> = self
                        .core_options
                        .iter()
                        .filter(|o| o.current != o.default)
                        .map(|o| (o.key.clone(), o.default.clone()))
                        .collect();
                    for (k, v) in changes {
                        self.set_option(&k, &v);
                    }
                    self.refresh_options();
                }
                UiAction::RestoreKeys(profile) => {
                    let key = profile.key(&self.core_id).to_owned();
                    self.options
                        .config
                        .set_keymap(&key, KeyMap::genesis_default());
                    self.config_dirty = true;
                    self.rebuild_keyboard();
                    self.overlay.set_notice(text::KEYS_RESTORED);
                }
                UiAction::UnlockConfig => {
                    self.options.config.unlock();
                    self.config_dirty = true;
                    self.save_config();
                    if !self.options.config.is_locked() {
                        self.overlay.osd.info(text::UNLOCK_DONE, Instant::now());
                    }
                }
            }
        }
    }

    fn hotkey(&mut self, event_loop: &ActiveEventLoop, action: HotkeyAction, pressed: bool) {
        let now = Instant::now();
        if action == HotkeyAction::FastForward {
            if self.speed.fast_forward != pressed && pressed {
                self.overlay.osd.info(text::FAST_FORWARD_ON, now);
            }
            self.speed.fast_forward = pressed;
            self.apply_speed();
            return;
        }
        if !pressed {
            return;
        }
        match action {
            HotkeyAction::Menu => {
                if self.egui.is_some() {
                    self.open_screen(Screen::Menu);
                } else {
                    // No egui (GL too old for it): keep the phase 1 behaviour.
                    event_loop.exit();
                }
            }
            HotkeyAction::TogglePause => {
                self.speed.paused = !self.speed.paused;
                if self.speed.paused {
                    self.keyboard.release_all();
                    self.flush_sram();
                }
                let msg = if self.speed.paused {
                    text::PAUSED
                } else {
                    text::RESUMED
                };
                self.overlay.osd.info(msg, now);
                self.apply_speed();
            }
            HotkeyAction::Reset => {
                self.emulator.reset();
                self.overlay.osd.info(text::RESET_DONE, now);
            }
            HotkeyAction::SaveState => self.save_state(),
            HotkeyAction::LoadState => {
                self.load_state();
            }
            HotkeyAction::PrevSlot | HotkeyAction::NextSlot => {
                let delta = if action == HotkeyAction::NextSlot {
                    1
                } else {
                    -1
                };
                self.slot = step_slot(self.slot, delta);
                let saved = self
                    .emulator
                    .state_info(self.slot)
                    .map(|(h, _)| text::format_unix_utc(h.created_unix));
                self.overlay
                    .osd
                    .info(text::slot_selected(self.slot, saved.as_deref()), now);
            }
            HotkeyAction::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                if let Some(w) = &self.window {
                    w.window()
                        .set_fullscreen(self.fullscreen.then_some(Fullscreen::Borderless(None)));
                }
            }
            HotkeyAction::ToggleStats => self.overlay.show_stats = !self.overlay.show_stats,
            HotkeyAction::FastForward => {}
        }
        self.request_redraw();
    }

    fn step(&mut self, event_loop: &ActiveEventLoop) {
        let status = match self.emulator.run_frame(&mut self.keyboard, &mut self.audio) {
            Ok(s) => s,
            Err(e) => return self.fail(event_loop, e),
        };
        if status.av_info_changed {
            if let Some(pacer) = &mut self.pacer {
                pacer.set_fps(self.emulator.av_info().timing.fps);
            }
            self.audio
                .set_input_rate(self.emulator.av_info().timing.sample_rate);
        }
        let now = Instant::now();
        if status.options_changed {
            tracing::info!("options du core modifiées par le core");
            if !self.core_options.is_empty() {
                self.refresh_options();
            }
        }
        for m in self.emulator.take_messages() {
            let level = match m.level {
                LogLevel::Error => OsdLevel::Error,
                LogLevel::Warn => OsdLevel::Warn,
                _ => OsdLevel::Info,
            };
            tracing::info!("core : {}", m.text);
            self.overlay.osd.push(m.text, level, m.duration, now);
        }
        if status.video_updated
            && let Some(w) = &mut self.window
            && let Err(e) = w.upload_frame(self.emulator.frame())
        {
            tracing::warn!("frame upload failed: {e}");
        }
        self.fps.update(now, self.emulator.frame_count());
        self.request_redraw();
        if now >= self.next_flush {
            self.next_flush = now + SRAM_FLUSH_INTERVAL;
            self.flush_sram();
        }
        let done = self
            .options
            .exit_after_frames
            .is_some_and(|n| self.emulator.frame_count() >= n);
        if status.shutdown_requested {
            event_loop.exit();
        } else if done {
            self.finish(event_loop);
        }
    }

    /// `--frames` reached: quit, or first open `--screen` and render a few egui passes for `--dump-window`.
    fn finish(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(screen) = self.options.screen {
            if self.egui.is_none() {
                tracing::warn!("interface egui indisponible : capture sans l'écran {screen:?}");
            }
            if let (Some(egui), Some(w)) = (&mut self.egui, &self.window) {
                egui.assume_focused(w.window());
            }
            self.open_screen(screen);
            self.finishing = Some(SCREEN_CAPTURE_PASSES);
        } else if self.options.dump_window.is_some() {
            self.finishing = Some(1);
        } else {
            event_loop.exit();
            return;
        }
        self.apply_speed();
    }

    fn scaling(&self) -> Scaling {
        match self.options.config.video.scaling {
            ScalingChoice::Integer => Scaling::Integer,
            ScalingChoice::Fit => Scaling::Fit,
        }
    }

    fn stats_view(&self) -> StatsView {
        let level = self.audio.level();
        let s = self.audio.stats();
        StatsView {
            fps: self.fps.fps,
            frame: self.emulator.frame_count(),
            buffered_ms: level.map(|l| l.buffered.as_millis()),
            target_ms: level.map(|l| l.target.as_millis()),
            underruns: s.underruns,
            dropped_samples: s.dropped_samples,
        }
    }

    /// One presented frame: egui pass → actions → `draw_frame` → egui paint → (capture) → `swap_buffers`.
    fn render(&mut self, event_loop: &ActiveEventLoop, capture: bool) {
        let now = Instant::now();
        self.overlay.osd.prune(now);
        self.ui_repaint_at = None;
        let mut actions = Vec::new();
        let mut painted = false;
        let stats = self.overlay.show_stats.then(|| self.stats_view());
        if self.overlay.visible()
            && let (Some(egui), Some(window)) = (&mut self.egui, &self.window)
        {
            let data = UiData {
                game: self.emulator.game_name(),
                core_id: &self.core_id,
                config: &self.options.config,
                slot: self.slot,
                slots: &self.slots,
                options: &self.core_options,
                categories: &self.categories,
                stats,
                now,
            };
            let overlay = &mut self.overlay;
            let delay = egui.run(window.window(), |ctx| {
                actions = ui::show(ctx, overlay, &data);
            });
            painted = true;
            let mut next = self.overlay.osd.next_expiry();
            if delay < Duration::from_secs(1) {
                let t = now + delay;
                next = Some(next.map_or(t, |n| n.min(t)));
            }
            self.ui_repaint_at = next;
        }
        self.apply_actions(event_loop, actions);

        let scaling = self.scaling();
        let aspect = self.emulator.av_info().geometry.display_aspect();
        let Some(w) = &mut self.window else {
            return;
        };
        let size = w.window().inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if let Err(e) = w.draw_frame(size, aspect, scaling) {
            tracing::warn!("draw failed: {e}");
            return;
        }
        if painted && let Some(egui) = &mut self.egui {
            egui.paint(w);
        }
        if capture && let Some(path) = &self.options.dump_window {
            match w.capture().and_then(|f| f.write_png(path)) {
                Ok(()) => tracing::info!("fenêtre capturée : {}", path.display()),
                Err(e) => tracing::warn!("capture de la fenêtre impossible : {e}"),
            }
        }
        if let Err(e) = w.swap_buffers() {
            tracing::warn!("swap_buffers failed: {e}");
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let av = self.emulator.av_info();
        let base = (
            av.geometry.base_width.max(1),
            av.geometry.base_height.max(1),
        );
        let scale = initial_scale(
            base,
            self.options
                .scale
                .unwrap_or(self.options.config.video.scale),
        );
        let size = PhysicalSize::new(base.0.saturating_mul(scale), base.1.saturating_mul(scale));
        let attrs = WindowAttributes::default()
            .with_title("emu-rust")
            .with_inner_size(size);
        let fps = av.timing.fps;
        let window = match GlWindow::new(event_loop, attrs) {
            Ok(w) => w,
            Err(e) => return self.fail(event_loop, e),
        };
        if let Err(e) = window.set_vsync(self.options.config.video.vsync) {
            tracing::warn!("vsync non réglable : {e}");
        }
        match EguiLayer::new(&window) {
            Ok(layer) => self.egui = Some(layer),
            Err(e) => tracing::warn!("interface egui indisponible (menu désactivé) : {e}"),
        }
        self.window = Some(window);
        let mode = match self.options.config.audio.sync {
            SyncChoice::Audio => SyncMode::Audio,
            SyncChoice::Clock => SyncMode::Clock,
        };
        self.pacer = Some(Pacer::new(fps, mode, Instant::now()));
        self.apply_speed();
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::KeyboardInput { event: key, .. } = &event {
            let code = match key.physical_key {
                PhysicalKey::Code(c) => Some(c),
                PhysicalKey::Unidentified(_) => None,
            };
            let pressed = key.state == ElementState::Pressed;
            if self.overlay.is_open() {
                if let Some(code) = code
                    && pressed
                {
                    if self.overlay.capture().is_some() {
                        if !key.repeat {
                            self.finish_capture(code);
                        }
                        return;
                    }
                    if code == self.options.config.hotkeys.menu {
                        if !key.repeat {
                            self.go_back();
                        }
                        return;
                    }
                }
                if self.overlay.capture().is_none()
                    && let (Some(egui), Some(w)) = (&mut self.egui, &self.window)
                {
                    egui.on_window_event(w.window(), &event);
                }
                self.request_redraw();
            } else if let Some(code) = code {
                if let Some(action) = self.options.config.hotkeys.action_for(code) {
                    if !key.repeat {
                        self.hotkey(event_loop, action, pressed);
                    }
                } else if !key.repeat || !pressed {
                    self.keyboard.handle_key(code, pressed);
                }
            }
            return;
        }
        if let (Some(egui), Some(w)) = (&mut self.egui, &self.window)
            && egui.on_window_event(w.window(), &event)
            && self.overlay.is_open()
        {
            w.window().request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(w) = &self.window {
                    w.resize(size);
                    w.window().request_redraw();
                }
            }
            WindowEvent::Focused(false) => self.keyboard.release_all(),
            WindowEvent::RedrawRequested if self.finishing.is_none() => {
                self.render(event_loop, false);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.pacer.is_none() || event_loop.exiting() {
            return;
        }
        // Deterministic capture: render directly, without relying on RedrawRequested timing.
        if let Some(left) = self.finishing {
            let last = left <= 1;
            self.render(event_loop, last);
            if last {
                event_loop.exit();
            } else {
                self.finishing = Some(left - 1);
            }
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        let now = Instant::now();
        if let Some(t) = self.ui_repaint_at
            && t <= now
        {
            self.ui_repaint_at = None;
            self.request_redraw();
        }
        // The pacer caps consecutive `Run`s at MAX_BURST; the counter is only a safety net.
        for _ in 0..=MAX_BURST {
            let now = Instant::now();
            let level = self.audio.level();
            let Some(pacer) = &mut self.pacer else {
                return;
            };
            let wake = |t: Instant| self.ui_repaint_at.map_or(t, |u| u.min(t));
            match pacer.poll(now, level) {
                PaceDecision::Run => {
                    self.step(event_loop);
                    if event_loop.exiting() || self.finishing.is_some() {
                        return;
                    }
                }
                PaceDecision::WaitUntil(t) => {
                    event_loop
                        .set_control_flow(ControlFlow::WaitUntil(wake(t.min(self.next_flush))));
                    return;
                }
                PaceDecision::Idle => {
                    let flow = self
                        .ui_repaint_at
                        .map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
                    event_loop.set_control_flow(flow);
                    return;
                }
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_scale_fits_menu_and_is_bounded() {
        assert_eq!(initial_scale((320, 224), 3), 3);
        assert_eq!(initial_scale((320, 224), 1), 3);
        assert_eq!(initial_scale((64, 48), 3), 10);
        assert_eq!(initial_scale((1, 1), 99), MAX_SCALE);
        assert_eq!(initial_scale((u32::MAX, u32::MAX), 0), MIN_SCALE);
    }
}
