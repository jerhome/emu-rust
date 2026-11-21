//! winit `ApplicationHandler` driving the emulator: `Pacer` scheduling, shortcuts, SRAM flushing, and the egui
//! overlay (menu open ⇒ emulation paused, keys go to egui instead of the game).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use emu_assets::cores::{CoreActions, CoreId, PadDevices};
use emu_library::InputDefault;
use emu_runtime::{
    AudioConfig, AudioOutput, ContentSpec, CoreOption, CoreOptionCategory, Emulator,
    EmulatorOptions, GlWindow, InputBackend, InputFrame, LogLevel, M3uLaunch, MAX_BURST,
    MediaPolicy, PULSE_FRAMES, PaceDecision, Pacer, STATE_SLOTS, Scaling, SyncMode,
    core_id_from_path,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, WindowAttributes, WindowId};

use crate::computer::dos;
use crate::computer::eightbit::{
    TapeAction, initial_keyboard_mode, joyport_forced_by_name, tape_key, toggle_joyport,
    validate_launch,
};
use crate::computer::input::{CaptureEffect, CaptureEvent, CaptureState, KeyboardMode, map_button};
use crate::computer::keymap::KeyRemap;
use crate::computer::session::{ComputerInputs, ComputerSession, KeyOutcome};
use crate::computer::vkbd::{
    KeyboardLayout, VirtualKeyboard, legend_for_layout_value, legend_for_variant,
};
use crate::config::{MAX_SCALE, MIN_SCALE};
use crate::config_computer::KeyboardModeChoice;
use crate::content::{content_spec, direct_rules, with_launch};
use crate::egui_layer::EguiLayer;
use crate::game_identity::{ResolvedGame, resolve_game_with, state_load_needs_confirmation};
use crate::game_options::{GameOptions, LoadedGameOptions, effective_core_options_with};
use crate::gamepad::{GamepadNotice, GamepadSettings, NavKey, PadBackend, PadMap};
use crate::hotkeys::{HotkeyAction, SpeedState, core_fast_forward_ratio, step_slot};
use crate::keyboard::{KeyMap, PadStyle, is_computer_profile};
use crate::launch_firmware::{GameFacts, firmware_setup_for};
use crate::pad_mode::{PAD_PORTS, PadMode};
use crate::shutdown;
use crate::ui::{
    self, AddedDiskView, CaptureOutcome, ComputerView, ConfirmLoad, DisksView, EightBitView,
    Featured, JoyportView, LaunchView, OsdLevel, Overlay, StatsView, UiAction, UiData,
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
        /// Save files the core wrote for this game (`game_saves`, design v3.0 §3.5.3); `None` when the core
        /// declares none or the game has no content name.
        core_files: Option<CoreGameFiles>,
    },
}

/// Files of `reset_core_game_files`: folder given to the core for this game (`GameIdentity::core_save_dir`, never
/// `<saves>/<core_id>` rebuilt here), name of the content handed to the core, `game_saves` patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoreGameFiles {
    pub dir: PathBuf,
    pub content_name: String,
    pub patterns: Vec<String>,
    /// Marker written by the reset when the core has a `save_family`, so that the next launch does not adopt
    /// another core's memory card again (`content::no_adopt_marker`).
    pub adopt_marker: Option<PathBuf>,
}

impl Restart {
    /// Deletes the working copies of the game and the save files the core wrote for it (`ResetDisks` only). The
    /// core must be unloaded. `true` when something was deleted.
    pub(crate) fn apply(&self) -> Result<bool, emu_runtime::RuntimeError> {
        match self {
            Restart::Plain => Ok(false),
            Restart::ResetDisks {
                saves_core_dir,
                game,
                work_dir,
                core_files,
            } => {
                let copies = match work_dir {
                    Some(dir) => emu_runtime::reset_work_dir(saves_core_dir, dir),
                    None => emu_runtime::reset_working_copies(saves_core_dir, game),
                };
                let files = match core_files {
                    Some(f) => {
                        emu_runtime::reset_core_game_files(&f.dir, &f.content_name, &f.patterns)
                    }
                    None => Ok(Vec::new()),
                };
                for path in files.as_deref().unwrap_or_default() {
                    tracing::info!("sauvegarde du core supprimée : {}", path.display());
                }
                if let Some(marker) = core_files.as_ref().and_then(|f| f.adopt_marker.as_ref()) {
                    if let Some(parent) = marker.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(marker, b"") {
                        tracing::warn!("marqueur de non-reprise non écrit : {e}");
                    }
                }
                let deleted_files = !files?.is_empty();
                Ok(copies? || deleted_files)
            }
        }
    }
}

/// Pad polling period while nothing else wakes the loop (paused game, open menu): gilrs does not wake winit.
const PAD_POLL_INTERVAL: Duration = Duration::from_millis(16);

/// Polling period while the backend exists but no pad is plugged in (hotplug detection only).
const PAD_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Loop wake-up period for the pad backend: fast with a pad attached, slow otherwise.
pub(crate) fn pad_poll_interval(has_pad: bool) -> Duration {
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
    /// Save files of the core deleted with the working copies (« Réinitialiser »).
    core_files: Option<CoreGameFiles>,
    states_warned: bool,
    /// Core reading DOS game folders (dosbox_pure): boot program and reset entries of the menu.
    dos: bool,
    /// The core reads the sticks (`cores.toml` `analog`, ADR 0016 amended; DualShock mode for `pad_devices`).
    analog: bool,
    /// `cores.toml` `pad_devices` of the core (PlayStation): the menu offers « Manette : Numérique / DualShock ».
    pad_devices: Option<PadDevices>,
    /// Current pad mode on ports 0 and 1; `None` without `pad_devices`.
    pad_mode: Option<PadMode>,
    featured: Option<Featured>,
    soundfont_hint: bool,
    /// System of CDs (`disc = true`): the disk menu and its messages say « CD ».
    disc: bool,
    /// Keys of the « Cassette » menu (`cores.toml` `actions`).
    tape_actions: CoreActions,
    /// CPC / C64 entries of the menus.
    eightbit: EightBitView,
    /// The core's fast-forward request forbids the user's toggle (`inhibit_toggle`).
    fast_load_inhibit: bool,
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
        let typed_id = CoreId::new(&core_id);
        let spec = match (&options.spec, &typed_id) {
            // Planned by the library (`plan_disc_launch`: the discs of a CD game).
            (Some(spec), _) => spec.clone(),
            (None, Some(id)) => {
                dos::spec_for_content(&options.content, &prefs, id).map_err(UiError::Content)?
            }
            (None, None) => content_spec(&core_id, &options.content, &prefs),
        };
        // The game's system (library entry, else the content's extension and header for the CLI): firmware
        // table, « Réglages » section, default input mode and « CD » menu (design v6.0 §3.5).
        let game_database = options.game_system.clone().or_else(|| {
            typed_id.as_ref().and_then(|id| {
                crate::launch_firmware::content_database(&prefs, id, options.content.first())
            })
        });
        let facts = GameFacts {
            database: game_database.as_deref(),
            region: options.region,
        };
        let no_game = spec == ContentSpec::NoGame;
        let working_copy = matches!(
            &spec,
            ContentSpec::Media { rules, .. } if rules.policy == MediaPolicy::WorkingCopy
        );
        let save_scope = crate::content::save_scope(&prefs, &core_id);
        let ResolvedGame {
            spec,
            identity,
            options_path: game_options_path,
            options_inherited,
            resolution_failed,
        } = resolve_game_with(&core_id, spec, &options.dirs, save_scope);
        let mut game_options = GameOptions::load(&game_options_path);
        let spec = with_default_start(spec, game_options.options.dos.start.as_deref());
        let launch_kind = typed_id
            .as_ref()
            .map_or(M3uLaunch::None, |id| crate::content::m3u_launch(&prefs, id));
        let (launch, launch_warning) = launch_text(
            options.launch.as_deref(),
            &game_options.options,
            launch_kind,
        )
        .map_err(UiError::Content)?;
        let spec = with_launch(spec, launch.as_deref(), launch_kind, || {
            direct_rules(&prefs, &core_id)
        });
        let frontend_options = typed_id
            .as_ref()
            .map(|id| prefs.frontend_options(id))
            .unwrap_or_default();
        let global_options = options
            .config
            .core_options
            .get(&core_id)
            .cloned()
            .unwrap_or_default();
        let firmware = firmware_setup_for(
            &core_id,
            options.content.first(),
            facts,
            &options.dirs,
            &options.config,
            &prefs,
            &mut game_options.options,
        );
        let game_options_dirty = firmware.acknowledged_now;
        let core_variables = effective_core_options_with(
            &[],
            &frontend_options,
            &firmware.computed,
            &global_options,
            &game_options.options.core_options,
        );
        let core_variables_for_legend = core_variables.clone();
        let identity_banners =
            identity_banners(&identity, &core_id, options_inherited, resolution_failed);
        // Memory card of another core of the family (design v7.0 §2.3), before the core reads its `.srm`.
        let adopted_card = match (&typed_id, no_game) {
            (Some(id), false) => crate::content::adopt_memory_card(
                &prefs,
                id,
                &options.dirs.saves,
                &identity.save_name,
            ),
            _ => None,
        };
        let pad_devices = typed_id.as_ref().and_then(|id| prefs.pad_devices(id));
        let launch_pad = crate::pad_mode::launch_pad_mode(
            pad_devices.as_ref(),
            options.pad_device.as_deref(),
            game_options.options.input.pad_device.as_deref(),
            options
                .config
                .input
                .pad_device
                .get(&core_id)
                .map(String::as_str),
        );
        if options.pad_device.is_some() && launch_pad.is_none() {
            tracing::warn!(
                "--pad-device sans effet : {core_id} ne déclare pas de types de manette"
            );
        }
        let mut emulator = Emulator::load_content_as(
            &options.core,
            spec,
            identity,
            EmulatorOptions {
                system_dir: options.dirs.system.clone(),
                save_dir: options.dirs.saves.clone(),
                state_dir: options.dirs.states.clone(),
                assets_dir: options.dirs.assets.clone(),
                core_variables,
                cache_dir: options.dirs.cache_dir(),
                provide_vfs: typed_id.as_ref().is_some_and(|id| prefs.provides_vfs(id)),
                restore_added_disks: true,
                core_save_scope: save_scope,
                fastforward_override: options.auto_fast_load
                    && options.config.computer.auto_fast_load
                    && typed_id
                        .as_ref()
                        .is_some_and(|id| prefs.fastforward_override(id)),
                variable_state_size: typed_id
                    .as_ref()
                    .is_some_and(|id| prefs.variable_state_size(id)),
                // Pad mode of the game (design v7.0 §2.4); empty = JOYPAD on every port, as in 6.0.
                port_devices: launch_pad
                    .map(|(_, id)| crate::pad_mode::port_devices(id))
                    .unwrap_or_default(),
            },
        )?;
        let dos_core = typed_id
            .as_ref()
            .is_some_and(|id| prefs.folder_policy(id).is_some());
        // A DualShock id the core did not accept (`port_devices` refused, JOYPAD kept): back to the digital pad.
        let mut pad_refused = false;
        let pad_mode = launch_pad.map(|(mode, id)| {
            if mode == PadMode::DualShock
                && !emulator.controller_types(0).iter().any(|t| t.id == id)
            {
                tracing::warn!("{core_id} : manette DualShock ({id}) refusée, manette numérique");
                pad_refused = true;
                PadMode::Digital
            } else {
                mode
            }
        });
        let table_system = typed_id.as_ref().and_then(|id| {
            crate::launch_firmware::firmware_system_for(&prefs, id, game_database.as_deref())
        });
        let featured = typed_id.as_ref().and_then(|id| {
            let system = table_system.clone()?;
            let keys = prefs.featured_options(id, &system);
            (!keys.is_empty()).then(|| Featured {
                title: text::featured_title(&system),
                keys,
            })
        });
        let soundfont_hint = dos_core && !options.dirs.system.join(dos::SOUNDFONT_FILE).exists();
        let dos_first_launch = dos_core
            && emulator
                .content_name()
                .is_some_and(|n| dos::first_launch(&emulator.identity().core_save_dir, n));
        let core_files = typed_id.as_ref().and_then(|id| {
            let patterns = prefs.game_save_patterns(id);
            let content_name = emulator.content_name()?.to_owned();
            let adopt_marker = prefs.save_family(id).map(|_| {
                crate::content::no_adopt_marker(
                    &options.dirs.saves,
                    id,
                    &emulator.identity().save_name,
                )
            });
            (!patterns.is_empty() || adopt_marker.is_some()).then(|| CoreGameFiles {
                dir: emulator.identity().core_save_dir.clone(),
                content_name,
                patterns,
                adopt_marker,
            })
        });
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
        let system_info = table_system.as_deref().and_then(|database| {
            emu_library::systems()
                .iter()
                .find(|s| s.database == database)
        });
        let system_input = system_info.map_or(InputDefault::Keyboard, |s| s.default_input);
        let disc = system_info.is_some_and(|s| s.disc);
        let computer = (options.computer || is_computer_profile(&core_id)).then(|| {
            let input = &game_options.options.input;
            let mode = initial_keyboard_mode(
                input.keyboard_mode,
                options.config.computer.keyboard_mode,
                system_input,
                no_game,
            );
            let layout_id = keyboard_layout_id(&core_id);
            // The PC legend follows the layout the core types with (computed from the language, or chosen).
            let layout_value = typed_id
                .as_ref()
                .and_then(|id| prefs.layout_option(id))
                .and_then(|opt| emulator_layout_value(&core_variables_for_legend, &opt.option));
            let legend = input
                .extra
                .get("keyboard_layout")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .or_else(|| layout_value.map(|v| legend_for_layout_value(&v).to_owned()))
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
                VirtualKeyboard::new(layout_id, &legend),
            );
            if let Some(layout) = KeyboardLayout::load(layout_id) {
                let user = options.config.computer.remap.get(layout_id);
                session.keyboard.set_remap(KeyRemap::for_layout(
                    &layout,
                    user.unwrap_or(&BTreeMap::new()),
                ));
            }
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
        if computer
            .as_ref()
            .is_some_and(|c| c.mode == KeyboardMode::Joystick)
        {
            overlay.osd.push(
                text::joystick_launch_hint(options.config.hotkeys.host_key),
                OsdLevel::Info,
                Duration::from_secs(8),
                now,
            );
        }
        if let Some(warning) = launch_warning {
            overlay
                .osd
                .push(warning, OsdLevel::Warn, Duration::from_secs(10), now);
        }
        if dos_first_launch {
            overlay.osd.push(
                text::DOS_FIRST_LAUNCH.to_owned(),
                OsdLevel::Info,
                Duration::from_secs(12),
                now,
            );
        }
        if let Some(notice) = firmware.notice {
            overlay
                .osd
                .push(notice, OsdLevel::Warn, Duration::from_secs(12), now);
        }
        if let Some(source) = adopted_card {
            overlay.osd.push(
                text::memory_card_adopted(&source),
                OsdLevel::Info,
                Duration::from_secs(8),
                now,
            );
        }
        if pad_refused {
            overlay.osd.push(
                text::PAD_MODE_REFUSED.to_owned(),
                OsdLevel::Warn,
                Duration::from_secs(8),
                now,
            );
        }
        for banner in identity_banners {
            overlay
                .osd
                .push(banner, OsdLevel::Warn, Duration::from_secs(12), now);
        }
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
            crate::GameContent::NoGame | crate::GameContent::Folder { .. } => Vec::new(),
        };
        let tape_actions = typed_id
            .as_ref()
            .map(|id| prefs.actions(id))
            .unwrap_or_default();
        let joyport_core = core_id.starts_with("vice_");
        let eightbit = EightBitView {
            tape: !tape_actions.is_empty(),
            joyport: (joyport_core && !no_game).then(|| JoyportView {
                port: joyport_value(&core_variables_for_legend),
                forced: content_files
                    .first()
                    .and_then(|f| joyport_forced_by_name(f)),
            }),
            launch: (launch_kind != M3uLaunch::None && !no_game).then(|| LaunchView {
                kind: launch_kind,
                current: game_options.options.media.launch.clone(),
            }),
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
            game_options_dirty,
            game_scope: false,
            frontend_options,
            disks_view: None,
            working_copy,
            content_files,
            core_files,
            states_warned: false,
            dos: dos_core,
            // A core with `pad_devices` reads the sticks in DualShock mode only (design v7.0 §3.5).
            analog: match pad_mode {
                Some(mode) => mode == PadMode::DualShock,
                None => typed_id.as_ref().is_some_and(|id| prefs.analog(id)),
            },
            pad_devices,
            pad_mode,
            featured,
            soundfont_hint,
            disc,
            tape_actions,
            eightbit,
            fast_load_inhibit: false,
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
            pad.set_analog(self.analog);
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
        let saves_core_dir = self.options.dirs.saves.join(&self.core_id);
        let work_dir = self.emulator.work_dir();
        let orphans = work_dir.as_ref().filter(|d| d.is_dir()).map(|dir| {
            emu_runtime::orphan_copies(&saves_core_dir, dir).unwrap_or_else(|e| {
                tracing::warn!("copies non utilisées non listées : {e}");
                Vec::new()
            })
        });
        let mut content = self.content_files.clone();
        self.disks_view = self.emulator.disks().map(|mut d| {
            let current = d.current();
            let added: Vec<AddedDiskView> = d
                .added()
                .into_iter()
                .map(|a| {
                    // A restored disk's source is in the list already: not proposed again.
                    content.extend(a.source.clone());
                    AddedDiskView {
                        name: a
                            .copy
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        modified: a.modified,
                        size: a.size,
                        source_missing: a.source.is_some() && !a.source_present,
                        inserted: a.index.is_some() && a.index == current,
                        copy: a.copy,
                    }
                })
                .collect();
            DisksView {
                slots: d.slots(),
                current,
                addable: addable_images(&content, &extensions),
                can_reset,
                added,
                orphans,
                disc: self.disc,
            }
        });
    }

    fn forget_disk(&mut self, copy: &Path, delete: bool) {
        let name = copy
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let result = self
            .emulator
            .disks()
            .map(|mut d| d.forget_added(copy, delete));
        match result {
            Some(Ok(())) => {
                tracing::info!("disquette ajoutée retirée : {}", copy.display());
                self.overlay.end_disk_dialog();
                self.overlay.set_notice(if delete {
                    text::disk_copy_deleted(&name)
                } else {
                    text::disk_forgotten(&name)
                });
            }
            Some(Err(e)) => {
                tracing::warn!("disquette ajoutée non retirée : {e}");
                self.overlay.end_disk_dialog();
                self.overlay.set_notice(text::disk_error(&e));
            }
            None => self.overlay.set_notice(text::NO_DISK_CONTROL),
        }
        // Indexes after the removed disk moved down by one.
        self.reset_last_disk();
        self.refresh_disks();
    }

    /// `media.last_disk` follows the inserted disk after the list changed.
    fn reset_last_disk(&mut self) {
        let current = self.emulator.disks().and_then(|mut d| d.current());
        let media = &mut self.game_options.options.media;
        if media.last_disk.is_some() && media.last_disk != current {
            media.last_disk = current;
            self.game_options_dirty = true;
        }
    }

    fn adopt_orphan(&mut self, name: &str) {
        let result = self.emulator.disks().map(|mut d| d.adopt_orphan(name));
        match result {
            Some(Ok(_)) => {
                self.overlay.end_disk_dialog();
                self.overlay.set_notice(text::disk_adopted(name));
            }
            Some(Err(e)) => {
                tracing::warn!("copie {name} non reprise : {e}");
                self.overlay.set_notice(text::disk_error(&e));
            }
            None => self.overlay.set_notice(text::NO_DISK_CONTROL),
        }
        self.refresh_disks();
    }

    fn clean_copies(&mut self, names: &[String]) {
        let saves_core_dir = self.options.dirs.saves.join(&self.core_id);
        let Some(dir) = self.emulator.work_dir() else {
            return;
        };
        match emu_runtime::remove_orphan_copies(&saves_core_dir, &dir, names) {
            Ok(bytes) => {
                tracing::info!("{} copie(s) non utilisée(s) supprimée(s)", names.len());
                self.overlay.end_disk_dialog();
                self.overlay
                    .set_notice(text::copies_removed(names.len(), bytes));
            }
            Err(e) => {
                tracing::warn!("nettoyage des copies : {e}");
                self.overlay.set_notice(text::clean_failed(&e));
            }
        }
        self.refresh_disks();
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
                    .info(text::media_inserted(self.disc, i, slots.len(), &label), now);
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

    /// « Cassette » (design v5.0 §2.3): the core's key is held a few frames once the game runs again.
    fn tape(&mut self, action: TapeAction) {
        let Some(key) = tape_key(&self.tape_actions, action) else {
            return;
        };
        self.emulator.pulse_key(key, PULSE_FRAMES);
        let msg = match action {
            TapeAction::Play => text::TAPE_PLAYING,
            TapeAction::Stop => text::TAPE_STOPPED,
            TapeAction::Rewind => text::TAPE_REWOUND,
        };
        self.overlay.osd.info(msg, Instant::now());
    }

    /// « Port joystick » (design v5.0 §2.5): applied by the core at its next variable update, kept for this game.
    fn toggle_joyport(&mut self) {
        let Some(joy) = &mut self.eightbit.joyport else {
            return;
        };
        if joy.forced.is_some() {
            return;
        }
        let next = toggle_joyport(if joy.port == 1 { "1" } else { "2" });
        if let Err(e) = self.emulator.set_option(JOYPORT_OPTION, next) {
            tracing::warn!("option {JOYPORT_OPTION} refusée : {e}");
            self.overlay
                .osd
                .error(text::option_failed(&e), Instant::now());
            return;
        }
        joy.port = if next == "1" { 1 } else { 2 };
        self.game_options
            .options
            .core_options
            .insert(JOYPORT_OPTION.to_owned(), next.to_owned());
        self.game_options_dirty = true;
        self.save_game_options();
        if !self.core_options.is_empty() {
            self.refresh_options();
        }
        self.overlay
            .osd
            .info(text::joyport_switched(next), Instant::now());
    }

    /// « Manette : Numérique / DualShock » (design v7.0 §2.4): applied at once to ports 0 and 1, kept for this game.
    /// A device the core refuses changes nothing (OSD).
    fn toggle_pad_mode(&mut self) {
        let (Some(devices), Some(current)) = (&self.pad_devices, self.pad_mode) else {
            return;
        };
        let next = current.toggled();
        let now = Instant::now();
        let Some(&id) = devices.modes.get(next.key()) else {
            self.overlay.osd.error(text::PAD_MODE_REFUSED, now);
            return;
        };
        for port in PAD_PORTS {
            if let Err(e) = self.emulator.set_port_device(port, id) {
                tracing::warn!("port {port} : périphérique {id} refusé : {e}");
                if port == PAD_PORTS[0] {
                    self.overlay.osd.error(text::PAD_MODE_REFUSED, now);
                    return;
                }
            }
        }
        self.pad_mode = Some(next);
        self.analog = next == PadMode::DualShock;
        if let Some(pad) = &mut self.gamepad {
            pad.set_analog(self.analog);
        }
        self.game_options.options.input.pad_device = Some(next.key().to_owned());
        self.game_options_dirty = true;
        self.save_game_options();
        self.overlay
            .osd
            .info(text::pad_mode_switched(self.analog), now);
    }

    /// « Commande de démarrage… » / « Programme à lancer… »: `[media] launch` of this game, used from the next
    /// launch (the running content keeps its playlist).
    fn set_launch(&mut self, launch: String) {
        let Some(view) = &mut self.eightbit.launch else {
            return;
        };
        if let Err(e) = validate_launch(view.kind, &launch) {
            self.overlay.set_notice(e);
            return;
        }
        let value = (!launch.is_empty()).then_some(launch);
        let notice = if value.is_some() {
            text::LAUNCH_SAVED
        } else {
            text::LAUNCH_CLEARED
        };
        view.current.clone_from(&value);
        self.game_options.options.media.launch = value;
        self.game_options_dirty = true;
        self.save_game_options();
        self.overlay.set_notice(notice);
    }

    /// The core asked for fast-forward or gave it back (env 64, design v5.0 §2.4).
    fn core_fast_forward_changed(&mut self) {
        let now = Instant::now();
        match self.emulator.fastforward_override() {
            Some(o) if o.fastforward => {
                let was = self.speed.core_fast_forward_ratio.is_some();
                self.speed.core_fast_forward_ratio = Some(core_fast_forward_ratio(o.ratio));
                self.fast_load_inhibit = o.inhibit_toggle;
                if o.inhibit_toggle {
                    self.speed.fast_forward = false;
                }
                if !was {
                    self.overlay.osd.info(text::FAST_LOAD_ON, now);
                }
            }
            _ => {
                if self.speed.core_fast_forward_ratio.take().is_some() {
                    self.overlay.osd.info(text::FAST_LOAD_OFF, now);
                }
                self.fast_load_inhibit = false;
            }
        }
        self.apply_speed();
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
                work_dir: self.emulator.work_dir(),
                core_files: self.core_files.clone(),
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
            ..self.speed
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

    /// Load of the selected slot asked from the menu (`from_menu`), a keyboard shortcut or the pad. A state that
    /// may come from another file of the same name asks first (default « Non »). `true` if the state was loaded.
    fn request_load_state(&mut self, from_menu: bool) -> bool {
        if !state_load_needs_confirmation(self.emulator.state_origin(self.slot)) {
            return self.load_state();
        }
        if self.egui.is_none() {
            // No menu to ask in (GL too old for egui): the safe answer.
            self.overlay
                .osd
                .error(text::STATE_LOAD_UNCONFIRMED, Instant::now());
            return false;
        }
        if !from_menu {
            self.open_screen(Screen::Menu);
        }
        let has_disks = self.emulator.disks().is_some();
        self.overlay.ask_load_confirmation(ConfirmLoad {
            slot: self.slot,
            from_menu,
            has_disks,
        });
        self.request_redraw();
        false
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
                UiAction::ChangeStartProgram => {
                    // dosbox_pure reads Shift in `retro_reset`: its boot menu shows even with an autoboot.
                    if let Some(shift) = emu_runtime::RetroKey::from_name("lshift") {
                        self.emulator.reset_with_keys(&[shift]);
                    }
                    self.overlay
                        .osd
                        .info(text::DOS_CHANGE_PROGRAM_DONE, Instant::now());
                    self.close_overlay();
                }
                UiAction::SelectSlot(s) => self.slot = s,
                UiAction::SaveState => self.save_state(),
                UiAction::LoadState => {
                    if self.request_load_state(true) {
                        self.close_overlay();
                    }
                }
                UiAction::ConfirmLoadState(slot) => {
                    self.slot = slot;
                    if self.load_state() {
                        self.close_overlay();
                    } else {
                        self.refresh_slots();
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
                        Some(Err(e)) => self.overlay.set_notice(text::disk_error(&e)),
                        None => self.overlay.set_notice(text::NO_DISK_CONTROL),
                    }
                    self.refresh_disks();
                }
                UiAction::ForgetDisk { copy, delete } => self.forget_disk(&copy, delete),
                UiAction::AdoptOrphan(name) => self.adopt_orphan(&name),
                UiAction::CleanCopies(names) => self.clean_copies(&names),
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
                UiAction::Tape(action) => {
                    self.tape(action);
                    self.close_overlay();
                }
                UiAction::ToggleJoyport => self.toggle_joyport(),
                UiAction::TogglePadMode => self.toggle_pad_mode(),
                UiAction::SetLaunch(launch) => self.set_launch(launch),
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
            if pressed && self.fast_load_inhibit {
                self.overlay.osd.info(text::FAST_FORWARD_INHIBITED, now);
                return;
            }
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
                self.request_load_state(false);
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
            // PS + L3: next disk on computers and on CD systems (design v7.0 §3.5; the 3DO core has no disk
            // interface: nothing to change).
            let allowed = match action {
                HotkeyAction::ToggleVirtualKeyboard => self.computer.is_some(),
                HotkeyAction::NextDisk => {
                    self.computer.is_some() || (self.disc && self.emulator.disks().is_some())
                }
                _ => true,
            };
            if !allowed {
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
        if status.fastforward_changed {
            self.core_fast_forward_changed();
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
                dos: self.dos,
                featured: self.featured.as_ref(),
                soundfont_hint: self.soundfont_hint,
                eightbit: &self.eightbit,
                pad_dualshock: self.pad_mode.map(|m| m == PadMode::DualShock),
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

/// OSD banners of a launch (design v2.1-identite-disques §3.2): failed resolution, identity notice (homonym
/// separated, registry unusable), options inherited from the shared 2.0 name.
fn identity_banners(
    identity: &emu_runtime::GameIdentity,
    core_id: &str,
    options_inherited: bool,
    resolution_failed: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    if resolution_failed {
        out.push(text::IDENTITY_FALLBACK.to_owned());
    }
    if let Some(notice) = &identity.notice {
        out.push(text::identity_notice(notice, identity.base_name(), core_id));
    }
    if options_inherited {
        out.push(text::options_inherited(identity.base_name()));
    }
    out
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

/// Virtual keyboard and key overrides of a computer core (`data/keyboards/<id>.toml`). Only the layouts of the
/// released systems: PC, CPC and C64 come with their versions.
fn keyboard_layout_id(core_id: &str) -> &'static str {
    match core_id {
        "puae" | "puae2021" => "amiga",
        "dosbox_pure" => "pc",
        "cap32" => "cpc",
        "vice_x64" | "vice_x64sc" => "c64",
        _ => "atari_st",
    }
}

/// VICE option of the port the RetroPad drives (design v5.0 §2.5).
const JOYPORT_OPTION: &str = "vice_joyport";

/// `vice_joyport` among the effective core options; the core's default is port 2.
fn joyport_value(variables: &[(String, String)]) -> u8 {
    match emulator_layout_value(variables, JOYPORT_OPTION).as_deref() {
        Some("1") => 1,
        _ => 2,
    }
}

/// Launch text of this session (design v5.0 §4): `--launch` (refused when invalid: the message is the error),
/// else `[media] launch` of the game (an invalid one is ignored with a warning, the game still starts).
fn launch_text(
    cli: Option<&str>,
    game: &GameOptions,
    kind: M3uLaunch,
) -> Result<(Option<String>, Option<String>), String> {
    if let Some(text) = cli {
        validate_launch(kind, text.trim()).map_err(|e| format!("--launch : {e}"))?;
        return Ok((Some(text.trim().to_owned()), None));
    }
    let Some(text) = game.media.launch.as_deref().map(str::trim) else {
        return Ok((None, None));
    };
    match validate_launch(kind, text) {
        Ok(()) => Ok((Some(text.to_owned()), None)),
        Err(e) => {
            tracing::warn!("[media] launch ignoré : {e}");
            Ok((None, Some(text::launch_ignored(&e))))
        }
    }
}

/// Value of the layout option among the effective core options (`None` when absent).
fn emulator_layout_value(variables: &[(String, String)], key: &str) -> Option<String> {
    variables
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// `[dos] start` of the game's options for a folder launched without an explicit program (« pour cette fois »
/// wins). A remembered program that left the folder is ignored (the core's menu then shows).
fn with_default_start(spec: ContentSpec, start: Option<&str>) -> ContentSpec {
    match spec {
        ContentSpec::Folder {
            dir,
            name,
            start: None,
        } => {
            let start = start
                .filter(|s| {
                    dos::root_executables(&dir)
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(s))
                })
                .map(str::to_owned);
            ContentSpec::Folder { dir, name, start }
        }
        other => other,
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
    fn identity_banners_of_a_launch() {
        let mut id = emu_runtime::GameIdentity::legacy("Game", Path::new("/s/hatari"));
        assert!(identity_banners(&id, "hatari", false, false).is_empty());
        assert_eq!(
            identity_banners(&id, "hatari", false, true),
            vec![text::IDENTITY_FALLBACK.to_owned()]
        );
        id.notice = Some(emu_runtime::IdentityNotice::Homonym {
            first: PathBuf::from("/jeux/a/Game.st"),
            sram_copied: true,
            legacy_states: true,
        });
        let banners = identity_banners(&id, "hatari", true, false);
        assert_eq!(banners.len(), 2);
        assert!(banners[0].starts_with("Deux jeux portent le même nom"));
        assert_eq!(banners[1], text::options_inherited("Game"));
    }

    #[test]
    fn amiga_cores_get_the_amiga_keyboard_and_its_remap() {
        use crate::computer::input::ComputerKeyboard;
        use emu_runtime::RetroKey;
        assert_eq!(keyboard_layout_id("puae"), "amiga");
        assert_eq!(keyboard_layout_id("puae2021"), "amiga");
        assert_eq!(keyboard_layout_id("hatari"), "atari_st");
        let layout = KeyboardLayout::load("amiga").unwrap();
        let key = |name| RetroKey::from_name(name).unwrap();
        let mut kb = ComputerKeyboard::default();
        kb.set_remap(KeyRemap::for_layout(&layout, &BTreeMap::new()));
        kb.handle_key(KeyCode::AltLeft, true, None);
        kb.handle_key(KeyCode::AltRight, true, None);
        assert!(kb.is_held(key("lsuper")) && kb.is_held(key("rsuper")));
        assert!(!kb.is_held(key("lalt")) && !kb.is_held(key("ralt")));
        // The user's table wins; other keys keep their positional default.
        let user = BTreeMap::from([("ContextMenu".to_owned(), "ralt".to_owned())]);
        let mut kb = ComputerKeyboard::default();
        kb.set_remap(KeyRemap::for_layout(&layout, &user));
        kb.handle_key(KeyCode::ContextMenu, true, None);
        kb.handle_key(KeyCode::KeyA, true, None);
        assert!(kb.is_held(key("ralt")) && kb.is_held(key("a")));
    }

    /// Design v5.0 §6: CPC and C64 keyboards and their `[remap]` (F1-F10 ⇒ CPC keypad, AltGr ⇒ RESTORE, `<` ⇒ £),
    /// identical on the three OSes; the positional map keeps the other keys.
    #[test]
    fn eight_bit_cores_get_their_keyboards_and_remaps() {
        use emu_runtime::RetroKey;
        assert_eq!(keyboard_layout_id("cap32"), "cpc");
        assert_eq!(keyboard_layout_id("vice_x64"), "c64");
        assert_eq!(keyboard_layout_id("vice_x64sc"), "c64");
        let key = |name| RetroKey::from_name(name).unwrap();
        let cpc = KeyboardLayout::load("cpc").unwrap();
        let remap = KeyRemap::for_layout(&cpc, &BTreeMap::new());
        assert_eq!(remap.map(KeyCode::F1), Some(key("kp1")));
        assert_eq!(remap.map(KeyCode::F10), Some(key("kp0")));
        assert_eq!(remap.map(KeyCode::AltLeft), Some(key("lalt")));
        assert_eq!(remap.map(KeyCode::KeyQ), Some(key("q")));
        let q = cpc
            .rows
            .iter()
            .flatten()
            .find(|k| k.key == key("q"))
            .unwrap();
        assert_eq!(q.legend("fr"), "A");
        let c64 = KeyboardLayout::load("c64").unwrap();
        let user = BTreeMap::from([("Backquote".to_owned(), "end".to_owned())]);
        let remap = KeyRemap::for_layout(&c64, &user);
        assert_eq!(remap.map(KeyCode::AltRight), Some(key("pageup")));
        assert_eq!(remap.map(KeyCode::IntlBackslash), Some(key("end")));
        assert_eq!(remap.map(KeyCode::Backquote), Some(key("end")));
        assert_eq!(remap.map(KeyCode::Escape), Some(key("escape")));
    }

    #[test]
    fn launch_text_from_cli_or_game_options() {
        let mut game = GameOptions::default();
        assert_eq!(
            launch_text(Some(" RUN\"X "), &game, M3uLaunch::Command).unwrap(),
            (Some("RUN\"X".to_owned()), None)
        );
        assert!(
            launch_text(Some("RUN"), &game, M3uLaunch::None)
                .unwrap_err()
                .starts_with("--launch")
        );
        assert_eq!(
            launch_text(None, &game, M3uLaunch::Program).unwrap(),
            (None, None)
        );
        game.media.launch = Some("INTRO".into());
        assert_eq!(
            launch_text(None, &game, M3uLaunch::Program).unwrap(),
            (Some("INTRO".to_owned()), None)
        );
        // A hand-edited invalid text never blocks the game.
        game.media.launch = Some("A:B".into());
        let (text, warning) = launch_text(None, &game, M3uLaunch::Program).unwrap();
        assert!(text.is_none() && warning.is_some());
        assert_eq!(joyport_value(&[("vice_joyport".into(), "1".into())]), 1);
        assert_eq!(joyport_value(&[]), 2);
    }

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
        std::fs::write(core.join("disk1.save0.adf"), b"s").unwrap();
        std::fs::write(core.join("other.save0.adf"), b"s").unwrap();
        let restart = Restart::ResetDisks {
            saves_core_dir: core.clone(),
            game: "Jeu".to_owned(),
            work_dir: Some(own.clone()),
            core_files: Some(CoreGameFiles {
                dir: core.clone(),
                content_name: "disk1".to_owned(),
                patterns: vec!["{game}.save*.adf".to_owned()],
                adopt_marker: None,
            }),
        };
        assert!(restart.apply().unwrap());
        assert!(!own.exists());
        assert!(other.join("a.st").exists());
        assert!(!core.join("disk1.save0.adf").exists());
        assert!(core.join("other.save0.adf").exists());
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
