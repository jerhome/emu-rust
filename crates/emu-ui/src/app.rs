//! winit `ApplicationHandler` driving the emulator: `Pacer` scheduling, shortcuts, SRAM flushing, and the egui
//! overlay (menu open ⇒ emulation paused, keys go to egui instead of the game).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use emu_assets::cores::CoreId;
use emu_runtime::{
    AudioConfig, AudioOutput, ContentSpec, CoreOption, CoreOptionCategory, Emulator,
    EmulatorOptions, GlWindow, InputBackend, InputFrame, LogLevel, MAX_BURST, MediaPolicy,
    PaceDecision, Pacer, STATE_SLOTS, Scaling, SyncMode, core_id_from_path,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, WindowAttributes, WindowId};

use crate::computer::input::{CaptureEffect, CaptureEvent, CaptureState, KeyboardMode, map_button};
use crate::computer::session::{ComputerInputs, ComputerSession, KeyOutcome};
use crate::computer::vkbd::{VirtualKeyboard, legend_for_variant};
use crate::config::{MAX_SCALE, MIN_SCALE};
use crate::config_computer::KeyboardModeChoice;
use crate::content::{content_spec, game_key};
use crate::egui_layer::EguiLayer;
use crate::game_options::{
    GameOptions, LoadedGameOptions, effective_core_options, game_options_path,
};
use crate::gamepad::{GamepadNotice, GamepadSettings, NavKey, PadBackend, PadMap};
use crate::hotkeys::{HotkeyAction, SpeedState, step_slot};
use crate::keyboard::{KeyMap, PadStyle, is_computer_profile};
use crate::shutdown;
use crate::ui::{
    self, CaptureOutcome, ComputerView, DisksView, OsdLevel, Overlay, StatsView, UiAction, UiData,
    apply_capture, profile_map, text,
};
use crate::{KeyboardInput, LaunchOptions, ScalingChoice, Screen, SyncChoice, UiError};

const SRAM_FLUSH_INTERVAL: Duration = Duration::from_secs(10);
/// Frames emulated before a `--screen` capture when `--frames` is absent.
const DEFAULT_SCREEN_FRAMES: u64 = 30;
/// egui passes rendered before a `--screen` capture (window auto-sizing needs a couple of passes).
const SCREEN_CAPTURE_PASSES: u32 = 4;
/// The initial window is grown (by whole scale steps) up to this size so the menu fits.
const MIN_WINDOW: (u32, u32) = (640, 480);

/// What a hosted game hands back to the library.
pub(crate) struct Detached {
    pub window: Option<GlWindow>,
    pub egui: Option<EguiLayer>,
    pub config: crate::Config,
    pub error: Option<UiError>,
    /// The game asked to be started again (library only).
    pub restart: Option<Restart>,
    /// The pad backend, kept by the library for the next game (gilrs is opened once per session).
    pub gamepad: Option<PadBackend>,
}

/// Why a hosted game ends to be started again by the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Restart {
    /// Options that need a restart.
    Plain,
    /// « Réinitialiser les disquettes »: delete `<saves_core_dir>/<game>.disks` first.
    ResetDisks {
        saves_core_dir: PathBuf,
        game: String,
        /// Folder actually used by this session (`<game> (n).disks` for a same-name game); `None` without
        /// working copies.
        work_dir: Option<PathBuf>,
    },
}

impl Restart {
    /// Deletes the working copies of the game (`ResetDisks` only). The core must be unloaded.
    pub(crate) fn apply(&self) -> Result<bool, emu_runtime::RuntimeError> {
        match self {
            Restart::Plain => Ok(false),
            Restart::ResetDisks {
                saves_core_dir,
                game,
                work_dir,
            } => match work_dir {
                Some(dir) => emu_runtime::reset_work_dir(saves_core_dir, dir),
                None => emu_runtime::reset_working_copies(saves_core_dir, game),
            },
        }
    }
}

/// Pad polling period while nothing else wakes the loop (paused game, open menu): gilrs does not wake winit.
const PAD_POLL_INTERVAL: Duration = Duration::from_millis(16);

/// Polling period while the backend exists but no pad is plugged in (hotplug detection only).
const PAD_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Loop wake-up period for the pad backend: fast with a pad attached, slow otherwise.
fn pad_poll_interval(has_pad: bool) -> Duration {
    if has_pad {
        PAD_POLL_INTERVAL
    } else {
        PAD_IDLE_POLL_INTERVAL
    }
}

/// Opens the pad backend for `run` or the library. On failure the app goes on with the keyboard alone and the
/// returned notice is shown once.
pub(crate) fn open_gamepad(
    allowed: bool,
    config: &crate::Config,
) -> (Option<PadBackend>, Option<&'static str>) {
    match crate::gamepad::open_backend(allowed, &config.input.gamepad) {
        Ok(pad) => (pad, None),
        Err(e) => {
            tracing::warn!("manettes indisponibles : {e}");
            (None, Some(text::GAMEPAD_UNAVAILABLE))
        }
    }
}

/// The game's input backend plus the pad, OR-combined (the pad never replaces the keyboard).
struct WithPad<'a> {
    inner: &'a mut dyn InputBackend,
    pad: Option<&'a mut PadBackend>,
}

impl InputBackend for WithPad<'_> {
    fn poll(&mut self, frame_index: u64, state: &mut InputFrame) {
        self.inner.poll(frame_index, state);
        if let Some(pad) = &mut self.pad {
            pad.poll(frame_index, state);
        }
    }
}

/// Mouse wheel: pixels per notch for touchpads reporting pixel deltas.
const WHEEL_PIXELS_PER_NOTCH: f64 = 40.0;
/// Most images listed by « ajouter une image du même dossier ».
const MAX_ADDABLE: usize = 64;

// Field order is the drop order: egui painter, window/context (with its renderer), then the emulator
// (ADR 0008). The painter is also destroyed explicitly while its context is alive (`teardown`).
pub(crate) struct App {
    egui: Option<EguiLayer>,
    window: Option<GlWindow>,
    emulator: Emulator,
    pacer: Option<Pacer>,
    keyboard: KeyboardInput,
    /// Pad backend (ADR 0016), next to the keyboard; `None` without pad support or with `--no-gamepad`.
    gamepad: Option<PadBackend>,
    /// PS + R2 held on the previous poll (fast-forward is a held action).
    pad_fast_forward: bool,
    /// The active keymap binds Ctrl/Alt/Super to a button: Ctrl/Alt shortcuts are disabled.
    modifier_bound: bool,
    /// Ctrl held (tracked from `ModifiersChanged`); enables the F-key-free shortcuts.
    ctrl: bool,
    /// Alt held (Alt+Enter = fullscreen).
    alt: bool,
    options: LaunchOptions,
    audio: AudioOutput,
    error: Option<UiError>,
    core_id: String,
    style: PadStyle,
    /// Key of the keyboard profile (core id, plus system suffix on multi-system cores).
    profile: String,
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
    /// Launched from the library: the window belongs to the host and quitting returns to the library.
    hosted: bool,
    /// Hosted game over (quit, core shutdown, error): the host detaches the window and drops the game.
    ended: bool,
    restart: Option<Restart>,
    /// Computer system (ADR 0015): host key, machine keyboard, mouse, virtual keyboard.
    computer: Option<ComputerSession>,
    /// Console: the host key is held (it acts as Ctrl for the 1.0 shortcuts).
    host_held: bool,
    game_options: LoadedGameOptions,
    game_options_path: PathBuf,
    game_options_dirty: bool,
    /// Core option changes go to this game's options.
    game_scope: bool,
    /// Global overrides + frontend defaults: what a game override is compared with.
    frontend_options: Vec<(String, String)>,
    disks_view: Option<DisksView>,
    working_copy: bool,
    /// The files the game was launched with (disk menu: « ajouter une image du même dossier »).
    content_files: Vec<PathBuf>,
    states_warned: bool,
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

/// Window size in logical pixels: height = `base_height × scale`, width from the display aspect (like the
/// renderer), so there are no black bars at the initial size.
fn window_size(base_height: u32, aspect: f32, scale: u32) -> (u32, u32) {
    // A core announcing an absurd aspect must not produce a gigantic window.
    let aspect = if aspect.is_finite() {
        aspect.clamp(0.25, 4.0)
    } else {
        1.0
    };
    let height = base_height.max(1).saturating_mul(scale);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to the u32 range
    let width = (f64::from(height) * f64::from(aspect))
        .round()
        .clamp(1.0, 16384.0) as u32;
    (width, height)
}

/// Initial scale: the configured one, grown until the window reaches `MIN_WINDOW` (bounded by `MAX_SCALE`).
fn initial_scale(base_height: u32, aspect: f32, wanted: u32) -> u32 {
    let mut scale = wanted.clamp(MIN_SCALE, MAX_SCALE);
    while scale < MAX_SCALE && {
        let (w, h) = window_size(base_height, aspect, scale);
        w < MIN_WINDOW.0 || h < MIN_WINDOW.1
    } {
        scale += 1;
    }
    scale
}

pub(crate) fn run(options: LaunchOptions) -> Result<(), UiError> {
    let event_loop = EventLoop::new().map_err(|e| UiError::EventLoop(e.to_string()))?;
    let mut app = App::build(options, false)?;
    let (pad, notice) = open_gamepad(app.options.gamepad, &app.options.config);
    app.set_gamepad(pad);
    if let Some(notice) = notice {
        app.warn(notice.to_owned());
    }
    let proxy = event_loop.create_proxy();
    shutdown::set_waker(Some(Box::new(move || {
        // Only fails once the loop is gone: nothing left to wake.
        let _ = proxy.send_event(());
    })));
    let mut standalone = Standalone {
        app: Some(app),
        outcome: None,
    };
    let result = event_loop.run_app(&mut standalone);
    shutdown::set_waker(None);
    standalone.shutdown();
    result.map_err(|e| UiError::EventLoop(e.to_string()))?;
    standalone.outcome.unwrap_or(Ok(()))
}

/// Standalone window (`--core/--rom`). It owns the game so that `exiting` can unload it: on macOS, ⌘Q / « Quitter »
/// ends the process from `applicationWillTerminate` without returning from `run_app`, and the core writes some
/// files only in `retro_unload_game` (Mega CD `.brm`).
struct Standalone {
    app: Option<App>,
    outcome: Option<Result<(), UiError>>,
}

impl Standalone {
    /// Writes everything and unloads the game; idempotent (`exiting`, then again after `run_app`).
    fn shutdown(&mut self) {
        let Some(mut app) = self.app.take() else {
            return;
        };
        app.teardown();
        app.flush_sram();
        app.save_config();
        app.log_audio_stats();
        let mut outcome = app.error.take().map_or(Ok(()), Err);
        if outcome.is_ok()
            && let Some(path) = &app.options.dump_last_frame
        {
            outcome = app.emulator.frame().write_png(path).map_err(UiError::from);
        }
        // `Drop for Emulator`: last SRAM flush, then `retro_unload_game`.
        drop(app);
        tracing::info!("jeu fermé, sauvegardes écrites");
        self.outcome = Some(outcome);
    }
}

impl ApplicationHandler for Standalone {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(app) = &mut self.app {
            app.resumed(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(app) = &mut self.app {
            app.window_event(event_loop, id, event);
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        if let Some(app) = &mut self.app {
            app.device_event(event_loop, id, event);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if shutdown::requested() {
            if !event_loop.exiting() {
                tracing::info!("arrêt demandé : fermeture du jeu");
                event_loop.exit();
            }
            return;
        }
        if let Some(app) = &mut self.app {
            app.about_to_wait(event_loop);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.shutdown();
    }
}

impl App {
    /// Loads the core and the game; no window yet (`resumed` creates one, or the library host lends its own
    /// through [`App::attach`]).
    pub(crate) fn build(mut options: LaunchOptions, hosted: bool) -> Result<Self, UiError> {
        let core_id = core_id_from_path(&options.core);
        let prefs = crate::cores_ui::preferences(&options.config.cores);
        let spec = content_spec(&core_id, &options.content, &prefs);
        let no_game = spec == ContentSpec::NoGame;
        let working_copy = matches!(
            &spec,
            ContentSpec::Media { rules, .. } if rules.policy == MediaPolicy::WorkingCopy
        );
        let game_options_path =
            game_options_path(&options.dirs.game_options, &core_id, &game_key(&spec));
        let game_options = GameOptions::load(&game_options_path);
        let frontend_options = CoreId::new(&core_id)
            .map(|id| prefs.frontend_options(&id))
            .unwrap_or_default();
        let core_variables = effective_core_options(
            &[],
            &frontend_options,
            &options
                .config
                .core_options
                .get(&core_id)
                .cloned()
                .unwrap_or_default(),
            &game_options.options.core_options,
        );
        let mut emulator = Emulator::load_content(
            &options.core,
            spec,
            EmulatorOptions {
                system_dir: options.dirs.system.clone(),
                save_dir: options.dirs.saves.clone(),
                state_dir: options.dirs.states.clone(),
                assets_dir: options.dirs.assets.clone(),
                core_variables,
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
        let style = PadStyle::detect(&core_id, options.content.first().unwrap_or(Path::new("")));
        let profile = style.profile_key(&core_id);
        let keymap = options.config.keymap_for(&profile, style);
        tracing::info!(
            "{} touches liées (profil {profile}, manette {style:?}) : {}",
            keymap.bindings().len(),
            style.describe(&keymap)
        );
        if options.dump_window.is_some() || options.screen.is_some() {
            options
                .exit_after_frames
                .get_or_insert(DEFAULT_SCREEN_FRAMES);
        }
        let now = Instant::now();
        let mut overlay = Overlay::default();
        let computer = (options.computer || is_computer_profile(&core_id)).then(|| {
            let input = &game_options.options.input;
            let mode = match input.keyboard_mode {
                Some(m) => m,
                None if no_game => KeyboardModeChoice::Keyboard,
                None => options.config.computer.default_keyboard_mode,
            };
            let legend = input
                .extra
                .get("keyboard_layout")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    legend_for_variant(
                        options
                            .config
                            .computer
                            .emutos_variant_or_system()
                            .as_deref(),
                    )
                    .to_owned()
                });
            let mut session = ComputerSession::new(
                options.config.hotkeys.host_key,
                KeyboardMode::from(mode),
                VirtualKeyboard::new("atari_st", &legend),
            );
            session.auto_capture = input
                .auto_capture
                .unwrap_or(options.config.computer.auto_capture);
            session.grab_allowed = options.config.computer.grab_pointer;
            session
                .mouse
                .set_sensitivity(options.config.computer.mouse_sensitivity);
            session
        });
        let hint = if computer.is_some() {
            text::host_hint(options.config.hotkeys.host_key)
        } else {
            text::menu_hint(options.config.hotkeys.menu)
        };
        overlay.osd.info(hint, now);
        if let Some(notice) = emulator.prepared_media().and_then(|p| p.notice.as_ref()) {
            overlay.osd.push(
                text::work_dir_notice(notice),
                OsdLevel::Warn,
                Duration::from_secs(10),
                now,
            );
        }
        if let Some(disk) = game_options.options.media.last_disk.filter(|d| *d > 0)
            && let Some(mut disks) = emulator.disks()
            && let Err(e) = disks.insert(disk)
        {
            tracing::warn!("disquette {} non réinsérée : {e}", disk + 1);
        }
        let content_files = match &options.content {
            crate::GameContent::Files(f) => f.clone(),
            crate::GameContent::NoGame => Vec::new(),
        };
        Ok(Self {
            egui: None,
            window: None,
            emulator,
            pacer: None,
            modifier_bound: keymap.binds_modifier(),
            keyboard: KeyboardInput::new(keymap),
            gamepad: None,
            pad_fast_forward: false,
            ctrl: false,
            alt: false,
            options,
            audio,
            error: None,
            core_id,
            style,
            profile,
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
            hosted,
            ended: false,
            restart: None,
            computer,
            host_held: false,
            game_options,
            game_options_path,
            game_options_dirty: false,
            game_scope: false,
            frontend_options,
            disks_view: None,
            working_copy,
            content_files,
            states_warned: false,
        })
    }

    fn log_audio_stats(&self) {
        let stats = self.audio.stats();
        tracing::info!(
            "audio stats: underruns={} dropped_samples={} stream_failed={} recoveries={}",
            stats.underruns,
            stats.dropped_samples,
            stats.stream_failed,
            stats.recoveries
        );
    }

    /// Hosted mode: takes the library's window (and egui layer) and starts pacing. The window keeps its size.
    pub(crate) fn attach(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: GlWindow,
        egui: Option<EguiLayer>,
    ) {
        if let Err(e) = window.set_vsync(self.options.config.video.vsync) {
            tracing::warn!("vsync non réglable : {e}");
        }
        let fps = self.emulator.av_info().timing.fps;
        window.window().request_redraw();
        self.window = Some(window);
        self.egui = egui;
        let mode = match self.options.config.audio.sync {
            SyncChoice::Audio => SyncMode::Audio,
            SyncChoice::Clock => SyncMode::Clock,
        };
        self.pacer = Some(Pacer::new(fps, mode, Instant::now()));
        self.apply_speed();
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    /// Hosted mode, game over: SRAM and config written, window and egui handed back with the (possibly edited)
    /// configuration and the error that ended the game, if any. The emulator is dropped by the caller.
    pub(crate) fn detach(&mut self) -> Detached {
        self.release_inputs();
        self.capture(CaptureEvent::FocusLost);
        self.audio.set_paused(true);
        self.flush_sram();
        self.save_config();
        self.save_game_options();
        self.log_audio_stats();
        self.pacer = None;
        Detached {
            window: self.window.take(),
            egui: self.egui.take(),
            config: self.options.config.clone(),
            error: self.error.take(),
            restart: self.restart.take(),
            gamepad: self.gamepad.take(),
        }
    }

    /// Gives the game the pad backend (opened by `run`, or lent by the library) with this game's map and the
    /// current settings. Presses made before the game started are dropped; connection notices are kept.
    pub(crate) fn set_gamepad(&mut self, pad: Option<PadBackend>) {
        let config = &self.options.config.input.gamepad;
        self.gamepad = pad.map(|mut pad| {
            pad.set_settings(GamepadSettings::from_config(config));
            pad.set_map(PadMap::from_config(self.style, &self.profile, config));
            pad.set_ui_capture(false);
            pad.pump();
            let _ = pad.take_hotkeys();
            let _ = pad.take_nav();
            pad
        });
        self.pad_fast_forward = false;
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended
    }

    /// OSD message shown when the game starts (BIOS warnings from the library launch).
    pub(crate) fn warn(&mut self, message: String) {
        self.overlay.osd.push(
            message,
            OsdLevel::Warn,
            Duration::from_secs(6),
            Instant::now(),
        );
    }

    /// Keys, buttons and held host actions released (menu, focus lost, pause, end of game).
    fn release_inputs(&mut self) {
        self.keyboard.release_all();
        self.host_held = false;
        if let Some(c) = &mut self.computer {
            for action in c.release_all(&mut self.keyboard) {
                if action == HotkeyAction::FastForward {
                    self.speed.fast_forward = false;
                }
            }
        }
    }

    /// Pointer capture transition, applied to the window.
    fn capture(&mut self, event: CaptureEvent) {
        let Some(c) = &mut self.computer else {
            return;
        };
        let effect = c.capture_event(event);
        self.apply_capture_effect(effect);
    }

    fn apply_capture_effect(&mut self, effect: CaptureEffect) {
        let now = Instant::now();
        let (Some(c), Some(w)) = (&mut self.computer, &self.window) else {
            return;
        };
        let window = w.window();
        match effect {
            CaptureEffect::None => {}
            CaptureEffect::Grab => {
                let locked = if !c.grab_allowed {
                    None
                } else if window.set_cursor_grab(CursorGrabMode::Locked).is_ok() {
                    Some(true)
                } else if window.set_cursor_grab(CursorGrabMode::Confined).is_ok() {
                    Some(false)
                } else {
                    None
                };
                // Hidden in the fallback too: the machine draws its own pointer.
                window.set_cursor_visible(false);
                c.grab_result(locked);
                tracing::info!("souris : {:?}", c.capture);
                let msg = if c.capture == CaptureState::Fallback {
                    text::MOUSE_FALLBACK.to_owned()
                } else {
                    text::mouse_captured(self.options.config.hotkeys.host_key)
                };
                self.overlay.osd.info(msg, now);
            }
            CaptureEffect::Ungrab => {
                if let Err(e) = window.set_cursor_grab(CursorGrabMode::None) {
                    tracing::warn!("libération de la souris : {e}");
                }
                window.set_cursor_visible(true);
                self.overlay.osd.info(text::MOUSE_RELEASED, now);
            }
        }
    }

    /// Writes `game-options/<core>/<game>.toml` if something changed; an invalid file is never overwritten.
    fn save_game_options(&mut self) {
        if !std::mem::take(&mut self.game_options_dirty) {
            return;
        }
        let loaded = &self.game_options;
        match loaded.options.save(&self.game_options_path, loaded.locked) {
            Ok(()) => tracing::info!(
                "options du jeu enregistrées : {}",
                self.game_options_path.display()
            ),
            Err(e) => {
                tracing::warn!("options du jeu non enregistrées : {e}");
                let msg = if loaded.locked {
                    text::GAME_OPTIONS_LOCKED.to_owned()
                } else {
                    text::game_options_failed(&e)
                };
                self.overlay.osd.error(msg, Instant::now());
            }
        }
    }

    /// Disk menu content; `None` without a disk control interface.
    fn refresh_disks(&mut self) {
        let extensions: Vec<String> = self
            .emulator
            .system_info()
            .valid_extensions
            .iter()
            .map(|e| e.to_ascii_lowercase())
            .collect();
        let can_reset = self.hosted && self.working_copy;
        let content = self.content_files.clone();
        self.disks_view = self.emulator.disks().map(|mut d| DisksView {
            slots: d.slots(),
            current: d.current(),
            addable: addable_images(&content, &extensions),
            can_reset,
        });
    }

    fn computer_view(&self) -> Option<ComputerView> {
        let c = self.computer.as_ref()?;
        Some(ComputerView {
            mode: c.mode,
            auto_capture: c.auto_capture,
            sensitivity: self.options.config.computer.mouse_sensitivity,
            host_key: self.options.config.hotkeys.host_key,
        })
    }

    fn insert_disk(&mut self, index: Option<u32>) {
        let now = Instant::now();
        let Some(mut disks) = self.emulator.disks() else {
            self.overlay.osd.push(
                text::NO_DISK_CONTROL,
                OsdLevel::Warn,
                Duration::from_secs(3),
                now,
            );
            return;
        };
        let result = match index {
            Some(i) => disks.insert(i).map(|()| i),
            None => disks.insert_next(),
        };
        match result {
            Ok(i) => {
                let slots = disks.slots();
                let label = slots
                    .iter()
                    .find(|s| s.index == i)
                    .map_or(String::new(), |s| s.label.clone());
                self.overlay
                    .osd
                    .info(text::disk_inserted(i, slots.len(), &label), now);
                if self.game_options.options.media.last_disk != Some(i) {
                    self.game_options.options.media.last_disk = Some(i);
                    self.game_options_dirty = true;
                }
            }
            Err(e) => {
                tracing::warn!("changement de disquette : {e}");
                self.overlay.osd.error(text::disk_failed(&e), now);
            }
        }
    }

    fn toggle_keyboard_mode(&mut self) {
        let Some(c) = &mut self.computer else {
            return;
        };
        let mode = c.toggle_mode(&mut self.keyboard);
        let (choice, msg) = match mode {
            KeyboardMode::Keyboard => (KeyboardModeChoice::Keyboard, text::MODE_SWITCHED_KEYBOARD),
            KeyboardMode::Joystick => (KeyboardModeChoice::Joystick, text::MODE_SWITCHED_JOYSTICK),
        };
        self.game_options.options.input.keyboard_mode = Some(choice);
        self.game_options_dirty = true;
        self.overlay.osd.info(msg, Instant::now());
    }

    fn toggle_virtual_keyboard(&mut self) {
        if self.egui.is_none() {
            return;
        }
        let Some(c) = &mut self.computer else {
            return;
        };
        let open = !c.vkbd.is_open();
        c.vkbd.set_open(open);
        if open {
            c.keyboard.release_all();
            self.keyboard.release_all();
            self.capture(CaptureEvent::MenuOpened);
        }
        self.request_redraw();
    }

    /// Hosted only: the library starts the game again (after deleting the working copies for `ResetDisks`).
    fn restart_game(&mut self, event_loop: &ActiveEventLoop, reset_disks: bool) {
        if !self.hosted {
            return;
        }
        self.restart = Some(if reset_disks {
            Restart::ResetDisks {
                saves_core_dir: self.options.dirs.saves.join(&self.core_id),
                game: self.emulator.game_name().to_owned(),
                work_dir: self
                    .emulator
                    .prepared_media()
                    .and_then(|p| p.work_dir.clone()),
            }
        } else {
            Restart::Plain
        });
        self.exit(event_loop);
    }

    fn warn_states_once(&mut self) {
        if self.emulator.prepared_media().is_some()
            && !std::mem::replace(&mut self.states_warned, true)
        {
            self.overlay.osd.push(
                text::STATES_DISKS_WARNING,
                OsdLevel::Warn,
                Duration::from_secs(8),
                Instant::now(),
            );
        }
    }

    /// Standalone: quits the program. Hosted: hands control back to the library.
    fn exit(&mut self, event_loop: &ActiveEventLoop) {
        if self.hosted {
            self.ended = true;
            self.finishing = None;
        } else {
            event_loop.exit();
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl Into<UiError>) {
        self.error = Some(error.into());
        self.exit(event_loop);
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
        if let Some(c) = &mut self.computer {
            c.keyboard.set_paused(self.speed.paused);
        }
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
        if screen == Screen::VirtualKeyboard {
            if !self.computer.as_ref().is_some_and(|c| c.vkbd.is_open()) {
                self.toggle_virtual_keyboard();
            }
            return;
        }
        if !self.overlay.is_open() {
            // The game must not keep a key held, nor fast-forward, while the menu has the keyboard.
            self.release_inputs();
            self.capture(CaptureEvent::MenuOpened);
            if let Some(c) = &mut self.computer {
                c.vkbd.set_open(false);
            }
            self.speed.fast_forward = false;
            self.flush_sram();
        }
        match screen {
            Screen::Menu => {
                self.refresh_slots();
                self.refresh_disks();
            }
            Screen::CoreOptions => self.refresh_options(),
            Screen::Disks => self.refresh_disks(),
            Screen::Keys | Screen::Computer | Screen::VirtualKeyboard => {}
        }
        self.overlay.open(screen);
        self.apply_speed();
        self.request_redraw();
    }

    fn close_overlay(&mut self) {
        self.overlay.close();
        self.save_config();
        self.save_game_options();
        self.apply_speed();
        self.request_redraw();
    }

    fn go_back(&mut self) {
        let before = self.overlay.screen();
        self.overlay.back();
        let after = self.overlay.screen();
        if before != after {
            self.save_config();
            self.save_game_options();
        }
        match after {
            None => self.close_overlay(),
            Some(Screen::Menu) if before != after => self.refresh_slots(),
            _ => {}
        }
        self.request_redraw();
    }

    fn rebuild_keyboard(&mut self) {
        let map = self.options.config.keymap_for(&self.profile, self.style);
        self.modifier_bound = map.binds_modifier();
        self.keyboard = KeyboardInput::new(map);
    }

    fn finish_capture(&mut self, key: KeyCode) {
        let Some((profile, port, button)) = self.overlay.capture() else {
            return;
        };
        let style = self.style;
        let config = &self.options.config;
        let mut map = profile_map(config, &self.profile, style, profile);
        let outcome = apply_capture(&mut map, &config.hotkeys, key, port, button);
        let notice = match outcome {
            CaptureOutcome::Cancelled => text::KEYS_CANCELLED.to_owned(),
            CaptureOutcome::Cleared => text::KEYS_CLEARED.to_owned(),
            CaptureOutcome::Refused(action) => text::keys_refused(key, action),
            CaptureOutcome::RefusedModifier => text::KEYS_REFUSED_MODIFIER.to_owned(),
            CaptureOutcome::Bound {
                moved_from: Some(from),
            } => text::keys_moved(style, key, from, (port, button)),
            CaptureOutcome::Bound { moved_from: None } => {
                text::keys_bound(style, key, port, button)
            }
        };
        if matches!(
            outcome,
            CaptureOutcome::Cleared | CaptureOutcome::Bound { .. }
        ) {
            let profile_key = profile.key(&self.profile).to_owned();
            self.options.config.set_keymap(&profile_key, map);
            self.config_dirty = true;
            self.rebuild_keyboard();
        }
        self.overlay.end_capture(notice);
        self.request_redraw();
    }

    fn save_state(&mut self) {
        self.warn_states_once();
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
        self.warn_states_once();
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

    /// Value an option has without this game's override: global setting, else frontend default, else core default.
    fn value_without_game(&self, key: &str) -> Option<String> {
        self.options
            .config
            .core_options
            .get(&self.core_id)
            .and_then(|m| m.get(key))
            .cloned()
            .or_else(|| {
                self.frontend_options
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
            })
            .or_else(|| {
                self.core_options
                    .iter()
                    .find(|o| o.key == key)
                    .map(|o| o.default.clone())
            })
    }

    fn set_option(&mut self, key: &str, value: &str) {
        if let Err(e) = self.emulator.set_option(key, value) {
            tracing::warn!("option {key} refusée : {e}");
            self.overlay
                .osd
                .error(text::option_failed(&e), Instant::now());
            return;
        }
        if self.game_scope {
            let base = self.value_without_game(key);
            let overrides = &mut self.game_options.options.core_options;
            if base.as_deref() == Some(value) {
                overrides.remove(key);
            } else {
                overrides.insert(key.to_owned(), value.to_owned());
            }
            self.game_options_dirty = true;
            return;
        }
        // « Tous les jeux »: this game follows the global value from now on.
        if self.game_options.options.core_options.remove(key).is_some() {
            self.game_options_dirty = true;
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
                UiAction::Quit => self.exit(event_loop),
                UiAction::SetOption { key, value } => {
                    self.set_option(&key, &value);
                    self.refresh_options();
                }
                UiAction::DefaultOptions if self.game_scope => {
                    let keys: Vec<String> = self
                        .game_options
                        .options
                        .core_options
                        .keys()
                        .cloned()
                        .collect();
                    for k in keys {
                        if let Some(v) = self.value_without_game(&k) {
                            self.set_option(&k, &v);
                        }
                    }
                    self.game_options.options.core_options.clear();
                    self.game_options_dirty = true;
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
                    let key = profile.key(&self.profile).to_owned();
                    self.options
                        .config
                        .set_keymap(&key, KeyMap::builtin_for(&key, self.style));
                    self.config_dirty = true;
                    self.rebuild_keyboard();
                    self.overlay.set_notice(text::KEYS_RESTORED);
                }
                UiAction::InsertDisk(i) => {
                    self.insert_disk(Some(i));
                    self.close_overlay();
                }
                UiAction::EjectDisk => {
                    if let Some(mut d) = self.emulator.disks()
                        && let Err(e) = d.eject()
                    {
                        self.overlay
                            .osd
                            .error(text::disk_failed(&e), Instant::now());
                    }
                    self.refresh_disks();
                }
                UiAction::AddDisk(path) => {
                    let result = self.emulator.disks().map(|mut d| d.add(&path));
                    match result {
                        Some(Ok(_)) => {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            self.content_files.push(path);
                            self.overlay.set_notice(text::disk_added(&name));
                        }
                        Some(Err(e)) => self.overlay.set_notice(text::disk_failed(&e)),
                        None => self.overlay.set_notice(text::NO_DISK_CONTROL),
                    }
                    self.refresh_disks();
                }
                UiAction::ResetDisks => self.restart_game(event_loop, true),
                UiAction::Restart => self.restart_game(event_loop, false),
                UiAction::ToggleVirtualKeyboard => {
                    self.close_overlay();
                    self.toggle_virtual_keyboard();
                }
                UiAction::ToggleKeyboardMode => self.toggle_keyboard_mode(),
                UiAction::SetAutoCapture(on) => {
                    if let Some(c) = &mut self.computer {
                        c.auto_capture = on;
                        self.game_options.options.input.auto_capture = Some(on);
                        self.game_options_dirty = true;
                    }
                }
                UiAction::StepSensitivity(delta) => {
                    let cfg = &mut self.options.config.computer;
                    #[allow(clippy::cast_precision_loss)] // delta is -1..=1
                    let next = (cfg.mouse_sensitivity + 0.1 * delta.clamp(-1, 1) as f32).clamp(
                        crate::computer::input::MIN_SENSITIVITY,
                        crate::computer::input::MAX_SENSITIVITY,
                    );
                    cfg.mouse_sensitivity = (next * 10.0).round() / 10.0;
                    if let Some(c) = &mut self.computer {
                        c.mouse.set_sensitivity(cfg.mouse_sensitivity);
                    }
                    self.config_dirty = true;
                }
                UiAction::SetGameScope(on) => self.game_scope = on,
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
                    self.exit(event_loop);
                }
            }
            HotkeyAction::TogglePause => {
                self.speed.paused = !self.speed.paused;
                if self.speed.paused {
                    self.release_inputs();
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
            HotkeyAction::ToggleVirtualKeyboard => self.toggle_virtual_keyboard(),
            HotkeyAction::ToggleMouseCapture => self.capture(CaptureEvent::Toggle),
            HotkeyAction::ToggleKeyboardMode => self.toggle_keyboard_mode(),
            HotkeyAction::NextDisk => self.insert_disk(None),
            HotkeyAction::FastForward => {}
        }
        self.request_redraw();
    }

    /// Drains the pad on every loop turn (also while paused or in a menu): notices, hotkeys, held
    /// fast-forward, and menu / virtual keyboard navigation.
    fn poll_gamepad(&mut self, event_loop: &ActiveEventLoop) {
        let vkbd_open = self.computer.as_ref().is_some_and(|c| c.vkbd.is_open());
        let menu_open = self.overlay.is_open();
        let Some(pad) = &mut self.gamepad else {
            return;
        };
        pad.set_ui_capture(menu_open || vkbd_open);
        pad.pump();
        let notices = pad.take_notices();
        let hotkeys = pad.take_hotkeys();
        let nav = pad.take_nav();
        let fast_forward = pad.fast_forward_held();
        let pause_on_disconnect = pad.settings().pause_on_disconnect;
        let now = Instant::now();
        for notice in notices {
            match notice {
                GamepadNotice::Connected { name, port } => {
                    tracing::info!("manette branchée sur le port {port}");
                    self.overlay
                        .osd
                        .info(text::gamepad_connected(&name, port), now);
                }
                GamepadNotice::Disconnected { port } => {
                    tracing::info!("manette du port {port} débranchée");
                    let paused_now = pause_on_disconnect && !self.speed.paused && !menu_open;
                    self.overlay.osd.push(
                        if paused_now {
                            text::GAMEPAD_PAUSED.to_owned()
                        } else {
                            text::gamepad_disconnected(port)
                        },
                        OsdLevel::Warn,
                        Duration::from_secs(4),
                        now,
                    );
                    if paused_now {
                        self.speed.paused = true;
                        self.release_inputs();
                        self.flush_sram();
                        self.apply_speed();
                    }
                }
            }
            self.request_redraw();
        }
        if fast_forward != self.pad_fast_forward {
            self.pad_fast_forward = fast_forward;
            self.hotkey(event_loop, HotkeyAction::FastForward, fast_forward);
        }
        for action in hotkeys {
            if event_loop.exiting() || self.ended {
                return;
            }
            let computer_only = matches!(
                action,
                HotkeyAction::ToggleVirtualKeyboard | HotkeyAction::NextDisk
            );
            if computer_only && self.computer.is_none() {
                continue;
            }
            if self.overlay.is_open() {
                // Like the keyboard: in a menu only the menu shortcut acts (back / close).
                if action == HotkeyAction::Menu && self.overlay.capture().is_none() {
                    self.go_back();
                }
                continue;
            }
            self.hotkey(event_loop, action, true);
        }
        if nav.is_empty() || self.overlay.capture().is_some() {
            return;
        }
        for key in nav {
            let egui_key = match key {
                NavKey::Up => egui::Key::ArrowUp,
                NavKey::Down => egui::Key::ArrowDown,
                NavKey::Left => egui::Key::ArrowLeft,
                NavKey::Right => egui::Key::ArrowRight,
                NavKey::Activate => egui::Key::Enter,
                NavKey::Back => {
                    if self.overlay.is_open() {
                        self.go_back();
                    } else if let Some(c) = &mut self.computer {
                        c.vkbd.set_open(false);
                    }
                    continue;
                }
            };
            if let Some(egui) = &mut self.egui {
                egui.inject_key(egui_key);
            }
        }
        self.request_redraw();
    }

    fn step(&mut self, event_loop: &ActiveEventLoop) {
        let pad = self.gamepad.as_mut();
        let result = match &mut self.computer {
            Some(session) => {
                let mut inputs = ComputerInputs {
                    pad: &mut self.keyboard,
                    session,
                };
                let mut all = WithPad {
                    inner: &mut inputs,
                    pad,
                };
                self.emulator.run_frame(&mut all, &mut self.audio)
            }
            None => {
                let mut all = WithPad {
                    inner: &mut self.keyboard,
                    pad,
                };
                self.emulator.run_frame(&mut all, &mut self.audio)
            }
        };
        let status = match result {
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
            self.exit(event_loop);
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
            if screen == Screen::VirtualKeyboard && self.computer.is_none() {
                tracing::warn!("clavier virtuel : système non ordinateur");
            }
            self.open_screen(screen);
            self.finishing = Some(SCREEN_CAPTURE_PASSES);
        } else if self.options.dump_window.is_some() {
            self.finishing = Some(1);
        } else {
            self.exit(event_loop);
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
            audio_status: self.audio.status(),
            recoveries: s.recoveries,
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
        let computer_view = self.computer_view();
        let vkbd_open = self.computer.as_ref().is_some_and(|c| c.vkbd.is_open());
        if (self.overlay.visible() || vkbd_open)
            && let (Some(egui), Some(window)) = (&mut self.egui, &self.window)
        {
            let data = UiData {
                game: self.emulator.game_name(),
                core_id: &self.profile,
                style: self.style,
                config: &self.options.config,
                slot: self.slot,
                slots: &self.slots,
                options: &self.core_options,
                categories: &self.categories,
                hosted: self.hosted,
                stats,
                now,
                computer: computer_view.as_ref(),
                disks: self.disks_view.as_ref(),
                game_scope: self.game_scope,
                game_overrides: &self.game_options.options.core_options,
            };
            self.overlay.vkbd_open = vkbd_open;
            let overlay = &mut self.overlay;
            let mut vkbd = self.computer.as_mut().map(|c| &mut c.vkbd);
            let delay = egui.run(window.window(), |ctx| {
                actions = ui::show(ctx, overlay, &data);
                if let Some(v) = &mut vkbd {
                    v.show(ctx);
                }
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
        let size = w.physical_size();
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

impl App {
    /// Mouse events of a computer game (not over a menu or the virtual keyboard).
    fn computer_pointer(&mut self, event: &WindowEvent) {
        let uses_mouse = self.emulator.uses_mouse();
        let Some(c) = &mut self.computer else {
            return;
        };
        let effect = match event {
            WindowEvent::MouseInput { state, button, .. } => match map_button(*button) {
                Some(b) => c.on_button(b, *state == ElementState::Pressed, uses_mouse),
                None => CaptureEffect::None,
            },
            WindowEvent::CursorMoved { position, .. } => {
                c.on_cursor_moved(position.x, position.y);
                CaptureEffect::None
            }
            WindowEvent::CursorLeft { .. } => {
                c.mouse.cursor_left();
                CaptureEffect::None
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (f64::from(*x), f64::from(*y)),
                    MouseScrollDelta::PixelDelta(p) => {
                        (p.x / WHEEL_PIXELS_PER_NOTCH, p.y / WHEEL_PIXELS_PER_NOTCH)
                    }
                };
                c.on_wheel(y, x);
                CaptureEffect::None
            }
            _ => CaptureEffect::None,
        };
        self.apply_capture_effect(effect);
    }
}

/// Directory entries examined at most (the folder may be huge).
const MAX_SCANNED: usize = 4096;

/// Images next to the game's files that the core accepts and that are not in the set yet (sorted, bounded).
fn addable_images(content: &[PathBuf], extensions: &[String]) -> Vec<PathBuf> {
    let Some(dir) = content.first().and_then(|f| f.parent()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .take(MAX_SCANNED)
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && !content.contains(p))
        .filter(|p| {
            p.extension().is_some_and(|e| {
                let e = e.to_string_lossy().to_ascii_lowercase();
                e != "m3u" && extensions.contains(&e)
            })
        })
        .collect();
    out.sort();
    out.truncate(MAX_ADDABLE);
    out
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let av = self.emulator.av_info();
        let base_height = av.geometry.base_height.max(1);
        let aspect = av.geometry.display_aspect();
        let scale = initial_scale(
            base_height,
            aspect,
            self.options
                .scale
                .unwrap_or(self.options.config.video.scale),
        );
        // Logical pixels: the window keeps the same apparent size on HiDPI screens.
        let (width, height) = window_size(base_height, aspect, scale);
        let size = LogicalSize::new(width, height);
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
        if let WindowEvent::ModifiersChanged(m) = &event {
            self.ctrl = m.state().control_key();
            self.alt = m.state().alt_key();
            if let Some(c) = &mut self.computer {
                c.set_modifiers(m.state());
            }
        }
        if let WindowEvent::KeyboardInput { event: key, .. } = &event {
            let code = match key.physical_key {
                PhysicalKey::Code(c) => Some(c),
                PhysicalKey::Unidentified(_) => None,
            };
            let pressed = key.state == ElementState::Pressed;
            let host_key = self.options.config.hotkeys.host_key;
            if self.overlay.is_open() && self.computer.is_some() && code == Some(host_key) {
                // Host key alone closes the menu it opened (its press went to the state machine before).
                if !pressed && self.overlay.capture().is_none() {
                    self.close_overlay();
                }
                return;
            }
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
            } else if let (Some(code), Some(c)) = (code, &mut self.computer) {
                if c.vkbd.is_open() && code == KeyCode::Escape && pressed {
                    c.vkbd.set_open(false);
                    self.request_redraw();
                    return;
                }
                let outcomes = c.on_key(
                    code,
                    pressed,
                    key.repeat,
                    key.text.as_deref(),
                    &mut self.keyboard,
                );
                for outcome in outcomes {
                    match outcome {
                        KeyOutcome::Action(action, down) => self.hotkey(event_loop, action, down),
                        KeyOutcome::ToUi => {
                            if let (Some(egui), Some(w)) = (&mut self.egui, &self.window) {
                                egui.on_window_event(w.window(), &event);
                            }
                            self.request_redraw();
                        }
                    }
                }
            } else if let Some(code) = code {
                if code == host_key {
                    self.host_held = pressed;
                }
                let ctrl = self.ctrl || self.host_held;
                let ctrl_action =
                    modifier_shortcut(ctrl, self.alt, self.modifier_bound, pressed, code);
                if let Some(action) = ctrl_action {
                    if !key.repeat {
                        self.hotkey(event_loop, action, pressed);
                    }
                } else if let Some(action) = self.options.config.hotkeys.action_for(code) {
                    if !key.repeat {
                        self.hotkey(event_loop, action, pressed);
                    }
                } else if !key.repeat || !pressed {
                    self.keyboard.handle_key(code, pressed);
                }
            }
            return;
        }
        // Pointer events are never consumed while nothing is drawn: forwarding them would queue them forever.
        let forward = self.overlay.visible()
            || matches!(
                event,
                WindowEvent::Resized(_)
                    | WindowEvent::ScaleFactorChanged { .. }
                    | WindowEvent::Focused(_)
            );
        let vkbd_open = self.computer.as_ref().is_some_and(|c| c.vkbd.is_open());
        let forward = forward || vkbd_open;
        let mut over_ui = false;
        if forward && let (Some(egui), Some(w)) = (&mut self.egui, &self.window) {
            if egui.on_window_event(w.window(), &event) && (self.overlay.is_open() || vkbd_open) {
                w.window().request_redraw();
            }
            over_ui = vkbd_open && egui.context().is_pointer_over_egui();
        }
        if !self.overlay.is_open() && !over_ui {
            self.computer_pointer(&event);
        }
        match event {
            WindowEvent::CloseRequested => self.exit(event_loop),
            WindowEvent::Resized(size) => {
                if let Some(w) = &self.window {
                    w.resize(size);
                    w.window().request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = &self.window {
                    w.scale_factor_changed();
                    w.window().request_redraw();
                }
            }
            WindowEvent::Focused(false) => {
                self.release_inputs();
                self.capture(CaptureEvent::FocusLost);
                // Not every backend sends ModifiersChanged on focus loss.
                self.ctrl = false;
                self.alt = false;
            }
            WindowEvent::RedrawRequested if self.finishing.is_none() => {
                self.render(event_loop, false);
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && !self.overlay.is_open()
            && let Some(c) = &mut self.computer
        {
            c.on_device_motion(dx, dy);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(c) = &mut self.computer {
            c.flush_keys();
        }
        if self.pacer.is_none() || event_loop.exiting() || self.ended {
            return;
        }
        // Deterministic capture: render directly, without relying on RedrawRequested timing.
        if let Some(left) = self.finishing {
            let last = left <= 1;
            self.render(event_loop, last);
            if last {
                self.exit(event_loop);
            } else {
                self.finishing = Some(left - 1);
            }
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        self.poll_gamepad(event_loop);
        if event_loop.exiting() || self.ended || self.pacer.is_none() {
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
            let pad_at = self
                .gamepad
                .as_ref()
                .map(|p| now + pad_poll_interval(!p.attached().is_empty()));
            let wake = |t: Instant| {
                let t = self.ui_repaint_at.map_or(t, |u| u.min(t));
                pad_at.map_or(t, |p| p.min(t))
            };
            match pacer.poll(now, level) {
                PaceDecision::Run => {
                    self.step(event_loop);
                    if event_loop.exiting() || self.finishing.is_some() || self.ended {
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
                        .map_or(pad_at, |u| Some(pad_at.map_or(u, |p| p.min(u))))
                        .map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
                    event_loop.set_control_flow(flow);
                    return;
                }
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()));
    }
}

/// Ctrl+key / Alt+Enter shortcut for a press, off while the keymap binds a modifier to a game button.
fn modifier_shortcut(
    ctrl: bool,
    alt: bool,
    modifier_bound: bool,
    pressed: bool,
    code: KeyCode,
) -> Option<HotkeyAction> {
    if modifier_bound || !pressed {
        return None;
    }
    if ctrl {
        HotkeyAction::from_ctrl_key(code)
    } else if alt && code == KeyCode::Enter {
        Some(HotkeyAction::ToggleFullscreen)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_poll_is_slow_without_pad() {
        assert_eq!(pad_poll_interval(true), PAD_POLL_INTERVAL);
        assert!(pad_poll_interval(false) > PAD_POLL_INTERVAL);
    }

    #[test]
    fn reset_disks_deletes_only_the_session_work_dir() {
        let root = std::env::temp_dir().join(format!("emu-ui-reset-{}", std::process::id()));
        let core = root.join("core");
        let own = core.join("Jeu (2).disks");
        let other = core.join("Jeu.disks");
        for d in [&own, &other] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("a.st"), b"x").unwrap();
        }
        let restart = Restart::ResetDisks {
            saves_core_dir: core.clone(),
            game: "Jeu".to_owned(),
            work_dir: Some(own.clone()),
        };
        restart.apply().unwrap();
        assert!(!own.exists());
        assert!(other.join("a.st").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn modifier_shortcuts_follow_keymap() {
        assert_eq!(
            modifier_shortcut(false, true, false, true, KeyCode::Enter),
            Some(HotkeyAction::ToggleFullscreen)
        );
        assert!(modifier_shortcut(true, false, false, true, KeyCode::KeyL).is_some());
        assert_eq!(
            modifier_shortcut(true, false, true, true, KeyCode::KeyL),
            None
        );
        assert_eq!(
            modifier_shortcut(false, true, true, true, KeyCode::Enter),
            None
        );
        assert_eq!(
            modifier_shortcut(true, false, false, false, KeyCode::KeyL),
            None
        );
    }

    #[test]
    fn initial_scale_fits_menu_and_is_bounded() {
        let a = 320.0 / 224.0;
        assert_eq!(initial_scale(224, a, 3), 3);
        assert_eq!(initial_scale(224, a, 1), 3);
        assert_eq!(initial_scale(48, 4.0 / 3.0, 3), 10);
        assert_eq!(initial_scale(1, 1.0, 99), MAX_SCALE);
        assert_eq!(initial_scale(u32::MAX, 1.0, 0), MIN_SCALE);
    }

    #[test]
    fn window_follows_display_aspect() {
        // NES: 256x240 pixels shown at 8:7 → 274 px wide per 240, not 256.
        assert_eq!(window_size(240, 8.0 / 7.0, 3), (823, 720));
        // Master System: 256x192 at 4:3.
        assert_eq!(window_size(192, 4.0 / 3.0, 3), (768, 576));
        // Mega Drive 320x224 at its own aspect: unchanged.
        assert_eq!(window_size(224, 320.0 / 224.0, 3), (960, 672));
        assert_eq!(window_size(0, 0.0, 1), (1, 1));
        assert_eq!(window_size(240, 1000.0, 1), (960, 240));
        assert_eq!(window_size(240, f32::NAN, 1), (240, 240));
        assert!(window_size(240, 4.0, 16).0 <= 16384);
    }
}
