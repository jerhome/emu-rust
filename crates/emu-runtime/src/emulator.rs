use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use emu_libretro::{
    AvInfo, ControllerType, Core, CoreConfig, CoreMessage, CoreOption, CoreOptionCategory,
    FastForwardOverride, FrontendHandler, GameSource, InputDevice, InputQuery, JOYPAD_ID_MASK,
    JoypadButton, KeyEvent, KeyModifiers, MemoryKind, RetroKey, SystemInfo, VideoFrame, mouse_id,
};

use crate::audio::{AudioSink, NullSink};
use crate::content::{self, Limits};
use crate::error::RuntimeError;
use crate::frame::{FrameRgba, convert_frame};
use std::collections::VecDeque;

use crate::identity::{GameIdentity, SaveScope, identity_request_for, resolve_game_identity};
use crate::input::{AnalogStick, InputBackend, InputFrame, KeyboardState, MouseState};
use crate::media::{ContentSpec, Disks, MediaRules, MultiDisc, PreparedMedia, prepare_media_with};
use crate::saves::{
    FileMode, STATE_SLOTS, StateFileError, StateHeader, StateOrigin, core_id_from_path,
    decode_state, encode_state, read_bounded, read_state_header, write_atomic,
};
use crate::snapshot::{
    SnapshotLimits, SnapshotStatus, snapshot_folder, snapshot_path, snapshot_status, validate_start,
};

/// Largest content accepted from an archive (declared or actually decompressed size): 512 MiB.
pub const MAX_CONTENT_SIZE: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorOptions {
    pub system_dir: PathBuf,
    /// Saves root: SRAM goes to `<save_dir>/<core_id>/<game>.srm`, and the core gets `<save_dir>/<core_id>/`
    /// as `GET_SAVE_DIRECTORY` (ADR 0011).
    pub save_dir: PathBuf,
    /// Savestates root: `<state_dir>/<core_id>/<game>.state<slot>`.
    pub state_dir: PathBuf,
    /// `GET_CORE_ASSETS_DIRECTORY`.
    pub assets_dir: PathBuf,
    /// Core option overrides (`key`, `value`).
    pub core_variables: Vec<(String, String)>,
    /// `<data>/cache`: folder snapshots go to `<cache_dir>/<core_id>/` (design v4.0 §3.2).
    pub cache_dir: PathBuf,
    /// `CoreConfig::provide_vfs` (`cores.toml` `vfs = true`, `CorePreferences::provides_vfs`).
    pub provide_vfs: bool,
    /// Re-add disks added from the menu in earlier sessions (design §2.3).
    pub restore_added_disks: bool,
    /// `cores.toml` `save_scope` (design v5.0 §3.2): `Game` forces the per-game sub-folder of the 2.1 identity
    /// (`IdentityRequest::scope`) for every game of the core. `Core`: 4.0 behaviour.
    pub core_save_scope: SaveScope,
    /// `CoreConfig::fastforward_override` (`cores.toml` `fastforward_override`, and `[computer] auto_fast_load`).
    pub fastforward_override: bool,
    /// `CoreConfig::variable_state_size` (`cores.toml` `variable_state_size`).
    pub variable_state_size: bool,
    /// `(port, device id)` set after `load_game` (design v7.0 §3.2), e.g. the DualShock id of the core. An id the
    /// core refuses (`LibretroError::UnknownControllerDevice`) is logged and `RETRO_DEVICE_JOYPAD` is kept.
    pub port_devices: Vec<(u32, u32)>,
}

/// Outcome of one emulated frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameStatus {
    /// `false` when the core duped the frame or did not call the video callback; `frame()` is unchanged.
    pub video_updated: bool,
    pub av_info_changed: bool,
    pub shutdown_requested: bool,
    /// `RunEvents::options_changed`: re-read `Emulator::options`.
    pub options_changed: bool,
    /// `Emulator::fastforward_override` changed during this frame (env 64, design v5.0 §2.4).
    pub fastforward_changed: bool,
}

/// Headless emulation session: one core, one game. Same threading rules as `emu_libretro::Core` (!Send).
pub struct Emulator {
    pub(crate) core: Core,
    /// Cached copy so `av_info()` is infallible; refreshed when the core reports a change.
    av_info: AvInfo,
    frame: FrameRgba,
    input: InputFrame,
    frame_count: u64,
    pub(crate) core_id: String,
    /// `identity.save_name`: SRAM, states, working-copy folder of `File` / `NoGame` sessions.
    pub(crate) game_name: String,
    pub(crate) identity: GameIdentity,
    /// Stem of the `GameSource::Path` handed to `retro_load_game`.
    content_name: Option<String>,
    pub(crate) save_root: PathBuf,
    state_root: PathBuf,
    /// SRAM as last restored or written; `flush_sram` writes only when the core's copy differs.
    sram_snapshot: Vec<u8>,
    /// Backs `GameSource::Path` for archives extracted for a `need_fullpath` core. Removed in `Drop` after the
    /// game is unloaded (Windows cannot delete a file the core still holds open).
    content_dir: Option<tempfile::TempDir>,
    /// `ContentSpec::Media` sessions; holds the private temp directory of a `Direct` set (dropped after the
    /// game is unloaded, see `Drop`).
    pub(crate) prepared: Option<PreparedMedia>,
    /// `None` for `File` / `NoGame`: disks added from the menu then get working copies.
    /// Rules of a `ContentSpec::Media` session (disks added from the menu follow them too).
    pub(crate) media_rules: Option<MediaRules>,
    pub(crate) limits: Limits,
    /// The core asked for `RETRO_DEVICE_KEYBOARD` / `RETRO_DEVICE_MOUSE` at least once.
    keyboard_polled: bool,
    mouse_polled: bool,
    /// Simulated key presses (`pulse_key`), played one after the other.
    pulses: KeyPulses,
    /// Last `SET_FASTFORWARDING_OVERRIDE` request reported by `Core::run` (env 64).
    fastforward: Option<FastForwardOverride>,
}

/// Most pulses waiting at once; more are dropped (a menu action repeated faster than it can be played).
const MAX_QUEUED_PULSES: usize = 16;

/// Longest hold of one pulse, in frames (one second at 60 Hz); longer requests are clamped.
const MAX_PULSE_FRAMES: u32 = 60;

/// Queue of [`Emulator::pulse_key`] requests. A pulse holds its key for `frames` frames, then spends one frame
/// released (the release event) so that two pulses of the same key are seen as two presses by polling cores.
#[derive(Debug, Default)]
struct KeyPulses {
    queue: VecDeque<(RetroKey, u32)>,
    active: Option<ActivePulse>,
}

#[derive(Debug, Clone, Copy)]
struct ActivePulse {
    key: RetroKey,
    /// Frames still to be played with the key held; 0 = this frame is the release frame.
    held_left: u32,
}

impl KeyPulses {
    fn push(&mut self, key: RetroKey, frames: u32) {
        if self.queue.len() >= MAX_QUEUED_PULSES {
            tracing::warn!(?key, "too many simulated key presses waiting: dropped");
            return;
        }
        self.queue
            .push_back((key, frames.clamp(2, MAX_PULSE_FRAMES)));
    }

    /// Applies the pulse of this frame to `input` (already polled from the backends): state forced to pressed,
    /// press event at the first frame, release event after the last one unless the user holds the key. No
    /// allocation once `key_events` has room for one more event.
    fn apply(&mut self, input: &mut InputFrame) {
        if self.active.is_none() {
            self.active = self.queue.pop_front().map(|(key, frames)| ActivePulse {
                key,
                held_left: frames,
            });
            if let Some(pulse) = self.active
                && !input.keyboard.is_pressed(pulse.key)
            {
                input.key_events.push(key_event(pulse.key, true));
            }
        }
        let Some(pulse) = self.active.as_mut() else {
            return;
        };
        if pulse.held_left > 0 {
            input.keyboard.set(pulse.key, true);
            pulse.held_left -= 1;
        } else {
            if !input.keyboard.is_pressed(pulse.key) {
                input.key_events.push(key_event(pulse.key, false));
            }
            self.active = None;
        }
    }
}

/// Records the env 64 request of a run (`None`: none during this run, the previous state holds); `true` when
/// the state changed.
fn update_fastforward(
    state: &mut Option<FastForwardOverride>,
    request: Option<FastForwardOverride>,
) -> bool {
    let changed = request.is_some_and(|ff| *state != Some(ff));
    if request.is_some() {
        *state = request;
    }
    changed
}

fn key_event(key: RetroKey, down: bool) -> KeyEvent {
    KeyEvent {
        down,
        key,
        character: 0,
        modifiers: KeyModifiers::default(),
    }
}

/// Copies the `.srm` file into the core's SRAM (right after `retro_load_game`); returns the resulting SRAM,
/// which counts as already flushed. A read error other than "not found" fails the load: carrying on would
/// overwrite the user's save with a blank one at the next flush.
/// `MultiDisc::DiskControl`: images 1.. go in after `retro_load_game` (image 0 is the content), tray untouched.
fn append_launch_disks(core: &mut Core, images: &[PathBuf]) -> Result<(), RuntimeError> {
    if images.len() < 2 {
        return Ok(());
    }
    let mut disks = core.disk_control().ok_or(RuntimeError::NoDiskControl)?;
    for (num, path) in (1u32..).zip(&images[1..]) {
        disks.add_image()?;
        if let Err(e) = disks.replace_image(num, Some(path)) {
            if let Err(e2) = disks.replace_image(num, None) {
                tracing::warn!("cannot remove disk slot {num}: {e2}");
            }
            return Err(e.into());
        }
    }
    Ok(())
}

fn restore_sram(core: &mut Core, path: &Path) -> Result<Vec<u8>, RuntimeError> {
    let Some(sram) = core.memory_mut(MemoryKind::SaveRam) else {
        return Ok(Vec::new());
    };
    // Margin over the core's SRAM; anything beyond is cut anyway.
    match read_bounded(path, sram.len() + STATE_READ_MARGIN) {
        Ok(saved) => {
            let n = saved.len().min(sram.len());
            sram[..n].copy_from_slice(&saved[..n]);
            if saved.len() > sram.len() {
                tracing::warn!(
                    "{} is larger than the core's SRAM (> {} bytes): truncated",
                    path.display(),
                    sram.len()
                );
            } else {
                tracing::info!("SRAM restored from {}", path.display());
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err(path)(e)),
    }
    Ok(sram.to_vec())
}

/// Slack over the expected size when reading `.srm` and state files (container header, small mismatches).
const STATE_READ_MARGIN: usize = 64 * 1024;

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> RuntimeError + '_ {
    move |source| RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    }
}

impl Emulator {
    /// Loads the core then the game (contract below, "Phase 2"). Fails with `AlreadyLoaded` if another
    /// session is alive in the process. Same as `load_content(core, ContentSpec::File(rom), options)`.
    pub fn load(core: &Path, rom: &Path, options: EmulatorOptions) -> Result<Self, RuntimeError> {
        Self::load_with_limits(
            core,
            ContentSpec::File(rom.to_path_buf()),
            options,
            Limits::DEFAULT,
        )
    }

    /// 2.0 entry point (ADR 0015): one file, a disk set (prepared by [`crate::prepare_media`] once the core is
    /// loaded, with the extensions the binary accepts) or no content at all.
    /// Resolves the game identity itself ([`crate::resolve_game_identity`]); if that fails, the 2.0 name is used
    /// (logged).
    pub fn load_content(
        core: &Path,
        content: ContentSpec,
        options: EmulatorOptions,
    ) -> Result<Self, RuntimeError> {
        Self::load_with_limits(core, content, options, Limits::DEFAULT)
    }

    /// 2.1 entry point: same as [`Self::load_content`] with an identity the caller resolved beforehand
    /// ([`crate::resolve_game_identity`], needed earlier for per-game options). `save_name` keys SRAM, states and
    /// the working-copy folder (`set.name` of a `ContentSpec::Media` is replaced by it); the core gets
    /// `identity.core_save_dir` as `GET_SAVE_DIRECTORY`. With `restore_added_disks`, disks added from the menu in
    /// earlier sessions are re-added after `retro_load_game` (design §2.3; a failure is logged, never fatal).
    pub fn load_content_as(
        core: &Path,
        content: ContentSpec,
        identity: GameIdentity,
        options: EmulatorOptions,
    ) -> Result<Self, RuntimeError> {
        Self::load_as(core, content, identity, options, Limits::DEFAULT)
    }

    pub(crate) fn load_with_limits(
        core_path: &Path,
        spec: ContentSpec,
        options: EmulatorOptions,
        limits: Limits,
    ) -> Result<Self, RuntimeError> {
        let core_id = core_id_from_path(core_path);
        let saves_core_dir = options.save_dir.join(&core_id);
        let states_core_dir = options.state_dir.join(&core_id);
        let mut request = identity_request_for(&spec, &saves_core_dir, &states_core_dir);
        request.scope = options.core_save_scope;
        let identity = resolve_game_identity(&request).unwrap_or_else(|e| {
            tracing::warn!("game identity not resolved ({e}): 2.0 name used");
            request.fallback_identity()
        });
        Self::load_as(core_path, spec, identity, options, limits)
    }

    fn load_as(
        core_path: &Path,
        spec: ContentSpec,
        identity: GameIdentity,
        options: EmulatorOptions,
        limits: Limits,
    ) -> Result<Self, RuntimeError> {
        let core_id = core_id_from_path(core_path);
        let game_name = identity.save_name.clone();
        let saves_core_dir = options.save_dir.join(&core_id);
        fs::create_dir_all(&saves_core_dir).map_err(io_err(&saves_core_dir))?;
        let core_save_dir = identity.core_save_dir.clone();
        fs::create_dir_all(&core_save_dir).map_err(io_err(&core_save_dir))?;
        let core_state_dir = options.state_dir.join(&core_id);
        fs::create_dir_all(&core_state_dir).map_err(io_err(&core_state_dir))?;
        let restore_added_disks = options.restore_added_disks;

        let config = CoreConfig {
            system_dir: options.system_dir,
            save_dir: core_save_dir,
            assets_dir: options.assets_dir,
            variables: options.core_variables,
            provide_vfs: options.provide_vfs,
            copy_dir: Some(options.cache_dir.join("core-copies")),
            fastforward_override: options.fastforward_override,
            variable_state_size: options.variable_state_size,
        };
        let mut core = Core::load(core_path, config)?;
        let mut content_dir = None;
        let mut prepared = None;
        let mut media_rules = None;
        let mut folder_content_name = None;
        let source = match spec {
            ContentSpec::File(rom) => {
                let content = content::prepare(&rom, core.system_info(), limits)?;
                content_dir = content.temp_dir;
                Some(content.source)
            }
            ContentSpec::Media { mut set, rules } => {
                set.name.clone_from(&game_name);
                let exts = &core.system_info().valid_extensions;
                // Working copies stay flat under `<saves>/<core_id>/` (`<save_name>.disks`), whatever the save
                // directory handed to the core (2.1 identity design §2.1).
                let media = prepare_media_with(&set, &rules, exts, &saves_core_dir, limits)?;
                let source = GameSource::Path(media.content.clone());
                prepared = Some(media);
                media_rules = Some(rules);
                Some(source)
            }
            ContentSpec::NoGame => None,
            ContentSpec::Folder { dir, start, .. } => {
                let snapshot = snapshot_path(&options.cache_dir.join(&core_id), &game_name);
                let path = prepare_folder(&dir, &snapshot, start.as_deref())?;
                folder_content_name = snapshot
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned());
                Some(GameSource::Path(path))
            }
        };
        // A folder's path ends with `.zip#START.EXE`: its stem is the snapshot's, not the path's.
        let content_name = folder_content_name.or_else(|| match &source {
            Some(GameSource::Path(path)) => {
                path.file_stem().map(|s| s.to_string_lossy().into_owned())
            }
            _ => None,
        });
        let sram_path = crate::saves::sram_path(&options.save_dir, &core_id, &game_name);
        let disk_control_launch = media_rules
            .as_ref()
            .is_some_and(|r| r.multi_disc == MultiDisc::DiskControl);
        let loaded = match source {
            Some(source) => core.load_game_from(source),
            None => core.load_no_game(),
        }
        .map_err(RuntimeError::from)
        .and_then(|av_info| {
            if let Some(media) = prepared.as_ref().filter(|_| disk_control_launch) {
                append_launch_disks(&mut core, &media.images)?;
            }
            Ok((av_info, restore_sram(&mut core, &sram_path)?))
        });
        let (av_info, sram_snapshot) = match loaded {
            Ok(loaded) => loaded,
            Err(e) => {
                // Unload the core before the temporary content goes away. No `Emulator` exists yet, so no
                // SRAM flush can overwrite a save file that failed to load.
                drop(core);
                if let Some(dir) = content_dir {
                    content::remove_temp_dir(dir);
                }
                drop(prepared);
                return Err(e);
            }
        };
        for &(port, id) in &options.port_devices {
            if let Err(e) = core.set_controller_device(port, id) {
                tracing::warn!("port {port}: device {id} refused, keeping the joypad: {e}");
            }
        }
        let info = core.system_info();
        tracing::info!(
            core = %info.library_name,
            version = %info.library_version,
            core_id = %core_id,
            game = %game_name,
            width = av_info.geometry.base_width,
            height = av_info.geometry.base_height,
            fps = av_info.timing.fps,
            sample_rate = av_info.timing.sample_rate,
            "game loaded"
        );
        let mut emulator = Self {
            core,
            av_info,
            frame: FrameRgba::default(),
            input: InputFrame::default(),
            frame_count: 0,
            core_id,
            game_name,
            identity,
            content_name,
            save_root: options.save_dir,
            state_root: options.state_dir,
            sram_snapshot,
            content_dir,
            prepared,
            media_rules,
            limits,
            keyboard_polled: false,
            mouse_polled: false,
            pulses: KeyPulses::default(),
            fastforward: None,
        };
        if restore_added_disks {
            crate::added::restore_added(&mut emulator);
        }
        Ok(emulator)
    }

    /// `None` when the core registered no disk control interface.
    pub fn disks(&mut self) -> Option<Disks<'_>> {
        self.core.disk_control()?;
        Some(Disks { emulator: self })
    }

    /// Working-copy folder of the session: `None` for a `Direct` set; else the prepared one, else
    /// `<saves>/<core_id>/<save_name>.disks`. Returned owned (the fallback is computed).
    pub fn work_dir(&self) -> Option<PathBuf> {
        crate::added::session_work_dir(self)
    }

    /// Images prepared at load time (`ContentSpec::Media`), for the disk menu and « réinitialiser ».
    pub fn prepared_media(&self) -> Option<&PreparedMedia> {
        self.prepared.as_ref()
    }

    /// The core registered a keyboard callback or polled `RETRO_DEVICE_KEYBOARD` at least once.
    pub fn uses_keyboard(&self) -> bool {
        self.keyboard_polled || self.core.has_keyboard_callback()
    }

    /// The core polled `RETRO_DEVICE_MOUSE` at least once (drives mouse capture in the UI).
    pub fn uses_mouse(&self) -> bool {
        self.mouse_polled
    }

    pub fn system_info(&self) -> &SystemInfo {
        self.core.system_info()
    }

    pub fn av_info(&self) -> &AvInfo {
        &self.av_info
    }

    /// Polls `input` (state cleared first), runs one `retro_run`, converts video into `frame()`, forwards audio
    /// to `audio`. No allocation per frame once the frame size is stable.
    pub fn run_frame(
        &mut self,
        input: &mut dyn InputBackend,
        audio: &mut dyn AudioSink,
    ) -> Result<FrameStatus, RuntimeError> {
        // Polled once before `retro_run` rather than in `input_poll`: deterministic even for cores that read
        // input without polling, and the backend sees exactly one call per frame. `clear` keeps the event
        // buffer's capacity (no allocation per frame).
        self.input.clear();
        input.poll(self.frame_count, &mut self.input);
        self.pulses.apply(&mut self.input);
        // The keyboard callback must not run inside `retro_run` (ADR 0015 §3): delivered first, in order.
        for event in &self.input.key_events {
            self.core.send_key_event(*event);
        }

        let mut bridge = Bridge {
            input: &self.input,
            audio,
            frame: &mut self.frame,
            video_updated: false,
            polled: Polled::default(),
        };
        let result = self.core.run(&mut bridge);
        let video_updated = bridge.video_updated;
        let polled = bridge.polled;
        self.keyboard_polled |= polled.keyboard;
        self.mouse_polled |= polled.mouse;
        // Relative motion belongs to this frame only (constant during it, gone afterwards).
        self.input.mouse = MouseState {
            buttons: self.input.mouse.buttons,
            ..MouseState::default()
        };
        let events = result?;
        self.frame_count += 1;

        if events.av_info_changed
            && let Some(av_info) = self.core.av_info()
        {
            self.av_info = *av_info;
        }
        let fastforward_changed = update_fastforward(&mut self.fastforward, events.fastforward);
        Ok(FrameStatus {
            video_updated,
            av_info_changed: events.av_info_changed,
            shutdown_requested: events.shutdown_requested,
            options_changed: events.options_changed,
            fastforward_changed,
        })
    }

    /// Last frame produced by the core (empty before the first video callback).
    pub fn frame(&self) -> &FrameRgba {
        &self.frame
    }

    /// Number of completed `run_frame` calls; the index passed to `InputBackend::poll`.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Resets the emulated machine; `frame_count` keeps increasing (it indexes frames of this session).
    pub fn reset(&mut self) {
        self.core.reset();
    }

    /// Resets through `Core::reset_with`: exactly the `held` keys read as pressed by `input_state(KEYBOARD)`
    /// during `retro_reset` (no pad, no mouse, no `send_key_event`); the frame input is untouched. Video reaches
    /// `frame()` as usual, audio is dropped. dosbox_pure: `LSHIFT` forces its start menu (« Changer de programme
    /// de démarrage », design v4.0 §2.2).
    pub fn reset_with_keys(&mut self, held: &[RetroKey]) {
        let mut keyboard = KeyboardState::default();
        for &key in held {
            keyboard.set(key, true);
        }
        let input = InputFrame {
            keyboard,
            ..InputFrame::default()
        };
        let mut audio = NullSink;
        let mut bridge = Bridge {
            input: &input,
            audio: &mut audio,
            frame: &mut self.frame,
            video_updated: false,
            polled: Polled::default(),
        };
        self.core.reset_with(&mut bridge);
        self.keyboard_polled |= bridge.polled.keyboard;
        self.mouse_polled |= bridge.polled.mouse;
    }

    // ---- Phase 2 (docs/design/phase2-frontend.md §2.2). Contract for `Emulator::load` from phase 2:
    // - `.zip` content: passed as is if the core lists `zip` in `valid_extensions` or sets `block_extract`;
    //   otherwise the first entry (archive order) whose extension the core accepts is extracted, in memory
    //   (`GameSource::Memory`, virtual path `<archive dir>/<entry file name>`) or, if `need_fullpath`, into a
    //   private temporary directory removed on drop. Entry names are never used as paths (basename only);
    //   entries over `MAX_CONTENT_SIZE` (declared or actually read) are refused.
    // - creates `<save_dir>/<core_id>/` and `<state_dir>/<core_id>/`, then restores SRAM from `sram_path()`
    //   right after `retro_load_game` (shorter file: prefix copied; longer: truncated + `warn!`).
    // - `Drop` calls `flush_sram` (error logged, never panics).

    /// `core_id_from_path` of the core library.
    pub fn core_id(&self) -> &str {
        &self.core_id
    }

    /// Name keying SRAM and states: [`GameIdentity::save_name`] (the 1.0 / 2.0 name unless the game has a homonym).
    pub fn game_name(&self) -> &str {
        &self.game_name
    }

    /// Identity the session was loaded with (`save_name`, `core_save_dir`).
    pub fn identity(&self) -> &GameIdentity {
        &self.identity
    }

    /// Stem of the path handed to `retro_load_game` (`{game}` of the 3.0 `game_saves` patterns): differs from
    /// `save_name` for a single image under a working copy (`disk1`). `None` for `NoGame` or an in-memory load.
    pub fn content_name(&self) -> Option<&str> {
        self.content_name.as_deref()
    }

    /// Origin of slot `slot` relative to the running game (`game_fp` of its header vs. the identity's).
    /// `None`: empty, unreadable or invalid slot.
    pub fn state_origin(&self, slot: u8) -> Option<StateOrigin> {
        let (header, _) = self.state_info(slot)?;
        Some(match (header.game_fp, self.identity.game_fp()) {
            (Some(saved), Some(current)) if saved == current => StateOrigin::Same,
            (Some(_), Some(_)) => StateOrigin::OtherContent,
            // No fingerprint of the running game (registry unusable): nothing to compare.
            (Some(_), None) => StateOrigin::Unknown,
            (None, _) if self.identity.has_homonyms() => StateOrigin::Ambiguous,
            (None, _) => StateOrigin::Unknown,
        })
    }

    /// Where SRAM is persisted (whether or not the core exposes any).
    pub fn sram_path(&self) -> PathBuf {
        crate::saves::sram_path(&self.save_root, &self.core_id, &self.game_name)
    }

    /// Path of savestate `slot`; `InvalidSlot` if `slot >= STATE_SLOTS`.
    pub fn state_path(&self, slot: u8) -> Result<PathBuf, RuntimeError> {
        if slot >= STATE_SLOTS {
            return Err(RuntimeError::InvalidSlot(slot));
        }
        Ok(crate::saves::state_path(
            &self.state_root,
            &self.core_id,
            &self.game_name,
            slot,
        ))
    }

    /// Writes SRAM with `write_atomic` if it differs from what was last loaded or written. Returns `true` if
    /// a file was written; `Ok(false)` when unchanged or the core exposes no SRAM.
    pub fn flush_sram(&mut self) -> Result<bool, RuntimeError> {
        let path = self.sram_path();
        let Some(sram) = self.core.memory(MemoryKind::SaveRam) else {
            return Ok(false);
        };
        if sram == self.sram_snapshot.as_slice() {
            return Ok(false);
        }
        write_atomic(&path, sram, FileMode::Default).map_err(io_err(&path))?;
        self.sram_snapshot.clear();
        self.sram_snapshot.extend_from_slice(sram);
        tracing::debug!("SRAM written to {}", path.display());
        Ok(true)
    }

    /// Core memory region (tests, future cheats / achievements).
    pub fn memory(&self, kind: MemoryKind) -> Option<&[u8]> {
        self.core.memory(kind)
    }

    /// Raw core serialization (no container): in-memory snapshots for tests and future rewind.
    pub fn serialize_state(&mut self) -> Result<Vec<u8>, RuntimeError> {
        let mut out = Vec::new();
        self.core.serialize(&mut out)?;
        Ok(out)
    }

    pub fn unserialize_state(&mut self, data: &[u8]) -> Result<(), RuntimeError> {
        self.core.unserialize(data)?;
        Ok(())
    }

    /// `encode_state` + `write_atomic` to `state_path(slot)`; returns the path.
    pub fn save_state(&mut self, slot: u8) -> Result<PathBuf, RuntimeError> {
        let path = self.state_path(slot)?;
        let payload = self.serialize_state()?;
        let header = StateHeader {
            core_id: self.core_id.clone(),
            core_version: self.core.system_info().library_version.clone(),
            game: self.game_name.clone(),
            created_unix: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            game_fp: self.identity.game_fp(),
        };
        write_atomic(&path, &encode_state(&header, &payload), FileMode::Default)
            .map_err(io_err(&path))?;
        tracing::info!(slot, "state saved to {}", path.display());
        Ok(path)
    }

    /// Reads, `decode_state`s (`WrongCore` if another core id), warns on core version mismatch, then
    /// unserializes. On any error the running game is left untouched.
    pub fn load_state(&mut self, slot: u8) -> Result<(), RuntimeError> {
        let path = self.state_path(slot)?;
        let max = self.core.serialize_size() + STATE_READ_MARGIN;
        let bytes = read_bounded(&path, max).map_err(io_err(&path))?;
        if bytes.len() > max {
            return Err(StateFileError::Truncated.into());
        }
        let (header, payload) = decode_state(&bytes)?;
        if header.core_id != self.core_id {
            return Err(StateFileError::WrongCore {
                expected: self.core_id.clone(),
                found: header.core_id,
            }
            .into());
        }
        let version = &self.core.system_info().library_version;
        if header.core_version != *version {
            tracing::warn!(
                saved = %header.core_version,
                current = %version,
                "savestate made with another core version; trying anyway"
            );
        }
        // A core may have partially applied a state it then rejects: put the current one back.
        let backup = self.serialize_state()?;
        if let Err(e) = self.core.unserialize(payload) {
            if let Err(restore) = self.core.unserialize(&backup) {
                tracing::error!("cannot restore the running state after a failed load: {restore}");
            }
            return Err(e.into());
        }
        tracing::info!(slot, "state loaded from {}", path.display());
        Ok(())
    }

    /// Header of slot `slot` and file modification time, for the menu; `None` if absent or unreadable.
    pub fn state_info(&self, slot: u8) -> Option<(StateHeader, SystemTime)> {
        let path = self.state_path(slot).ok()?;
        let file = fs::File::open(&path).ok()?;
        let modified = file.metadata().and_then(|m| m.modified()).ok()?;
        let header = read_state_header(io::BufReader::new(file)).ok()?;
        Some((header, modified))
    }

    pub fn options(&mut self) -> Vec<CoreOption> {
        self.core.options()
    }

    pub fn option_categories(&self) -> Vec<CoreOptionCategory> {
        self.core.option_categories()
    }

    /// Device types declared by the core for `port` (`SET_CONTROLLER_INFO`); empty when none.
    pub fn controller_types(&self, port: u32) -> &[ControllerType] {
        self.core.controller_types(port)
    }

    /// Switches `port` to the device `id` (`Core::set_controller_device`), e.g. a DualShock; applied at once.
    /// An id the core did not declare is `LibretroError::UnknownControllerDevice`, nothing changes.
    pub fn set_port_device(&mut self, port: u32, id: u32) -> Result<(), RuntimeError> {
        Ok(self.core.set_controller_device(port, id)?)
    }

    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), RuntimeError> {
        self.core.set_option(key, value)?;
        Ok(())
    }

    /// Queues a simulated key press (menu « Cassette », design v5.0 §2.3): during `frames` frames (at least 2)
    /// `KeyboardState` reports `key` pressed; a press `KeyEvent` is delivered at the start and a release at the
    /// end, through the same path as `InputFrame::key_events` (outside `retro_run`, ADR 0015). Queued pulses
    /// follow one another without overlapping; a key really held by the user is not released by the pulse.
    /// Never goes through the host key nor the remapping. Usual value: [`crate::PULSE_FRAMES`]. `frames` is clamped to 2..=60.
    /// Played from the next `run_frame`: the release event comes with the frame after the last held one, and
    /// the next queued pulse starts one frame later. At most 16 pulses wait at once (more are dropped, logged).
    pub fn pulse_key(&mut self, key: RetroKey, frames: u32) {
        self.pulses.push(key, frames);
    }

    /// Last fast-forward request of the core (env 64) as reported by the runs so far (a request sent while
    /// loading is reported by the first `run_frame`); `None` without one or without
    /// `EmulatorOptions::fastforward_override`.
    pub fn fastforward_override(&self) -> Option<FastForwardOverride> {
        self.fastforward
    }

    /// Forwarded to the core (`GET_FASTFORWARDING`).
    pub fn set_fast_forwarding(&mut self, enabled: bool) {
        self.core.set_fast_forwarding(enabled);
    }

    /// OSD messages from the core since the last call.
    pub fn take_messages(&mut self) -> Vec<CoreMessage> {
        self.core.take_messages()
    }
}

/// Path handed to the core for a game folder: the snapshot (built first unless `Current`, default limits, no
/// progress), followed by `#START.EXE` when a start program is given (validated against `dir` first).
fn prepare_folder(
    dir: &Path,
    snapshot: &Path,
    start: Option<&str>,
) -> Result<PathBuf, RuntimeError> {
    let start = start.map(|s| validate_start(dir, s)).transpose()?;
    if snapshot_status(dir, snapshot)? != SnapshotStatus::Current {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        snapshot_folder(
            dir,
            snapshot,
            &SnapshotLimits::DEFAULT,
            &mut |_, _| {},
            &cancel,
        )?;
    }
    let mut path = snapshot.as_os_str().to_owned();
    if let Some(start) = start {
        path.push("#");
        path.push(start);
    }
    Ok(PathBuf::from(path))
}

impl Drop for Emulator {
    fn drop(&mut self) {
        if let Err(e) = self.flush_sram() {
            tracing::error!("SRAM not saved: {e}");
        }
        // Before any temporary content (`content_dir`, `prepared`) goes away: Windows cannot delete a file the
        // core still holds open.
        self.core.unload_game();
        if let Some(dir) = self.content_dir.take() {
            content::remove_temp_dir(dir);
        }
    }
}

/// Per-`run_frame` callback target; borrows everything so nothing is allocated.
struct Bridge<'a> {
    input: &'a InputFrame,
    audio: &'a mut dyn AudioSink,
    frame: &'a mut FrameRgba,
    video_updated: bool,
    polled: Polled,
}

/// Device classes the core asked about during one `retro_run`.
#[derive(Debug, Clone, Copy, Default)]
struct Polled {
    keyboard: bool,
    mouse: bool,
}

impl FrontendHandler for Bridge<'_> {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
        if let Some(frame) = frame {
            convert_frame(&frame, self.frame);
            self.video_updated = true;
        }
    }

    fn audio_samples(&mut self, samples: &[i16]) {
        self.audio.push_samples(samples);
    }

    fn input_poll(&mut self) {}

    fn input_state(&mut self, query: InputQuery) -> i16 {
        match query.device {
            InputDevice::Keyboard => self.polled.keyboard = true,
            InputDevice::Mouse => self.polled.mouse = true,
            _ => {}
        }
        input_state(self.input, query)
    }
}

fn input_state(input: &InputFrame, query: InputQuery) -> i16 {
    match query.device {
        InputDevice::Joypad => joypad_state(input, query),
        // One keyboard, whatever the port.
        InputDevice::Keyboard => u16::try_from(query.id)
            .ok()
            .and_then(RetroKey::new)
            .map_or(0, |key| i16::from(input.keyboard.is_pressed(key))),
        // Computers have a single mouse, on port 0.
        InputDevice::Mouse if query.port == 0 => mouse_state(&input.mouse, query.id),
        InputDevice::Analog => analog_state(input, query),
        _ => 0,
    }
}

/// `RETRO_DEVICE_INDEX_ANALOG_BUTTON`.
const INDEX_ANALOG_BUTTON: u32 = 2;
const ANALOG_ID_X: u32 = 0;
const ANALOG_ID_Y: u32 = 1;

/// Sticks of the port's `PadState` (0 unless a gamepad backend set them: keyboard only ⇒ 0) and analog buttons
/// derived from the digital ones (0x7FFF when pressed).
fn analog_state(input: &InputFrame, query: InputQuery) -> i16 {
    let Some(pad) = usize::try_from(query.port)
        .ok()
        .and_then(|port| input.pads.get(port))
    else {
        return 0;
    };
    if query.index == INDEX_ANALOG_BUTTON {
        return if query.id < 16 && pad.bits() & (1 << query.id) != 0 {
            i16::MAX
        } else {
            0
        };
    }
    let Some(stick) = AnalogStick::from_index(query.index) else {
        return 0;
    };
    let (x, y) = pad.analog(stick);
    match query.id {
        ANALOG_ID_X => x,
        ANALOG_ID_Y => y,
        _ => 0,
    }
}

fn mouse_state(mouse: &MouseState, id: u32) -> i16 {
    use crate::input::MouseButton;
    let axis = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    let button = |b: MouseButton| i16::from(mouse.is_pressed(b));
    match id {
        mouse_id::X => axis(mouse.dx),
        mouse_id::Y => axis(mouse.dy),
        mouse_id::LEFT => button(MouseButton::Left),
        mouse_id::RIGHT => button(MouseButton::Right),
        mouse_id::MIDDLE => button(MouseButton::Middle),
        mouse_id::BUTTON_4 => button(MouseButton::Button4),
        mouse_id::BUTTON_5 => button(MouseButton::Button5),
        mouse_id::WHEEL_UP => i16::from(mouse.wheel > 0),
        mouse_id::WHEEL_DOWN => i16::from(mouse.wheel < 0),
        mouse_id::HORIZ_WHEEL_UP => i16::from(mouse.hwheel > 0),
        mouse_id::HORIZ_WHEEL_DOWN => i16::from(mouse.hwheel < 0),
        _ => 0,
    }
}

fn joypad_state(input: &InputFrame, query: InputQuery) -> i16 {
    let Some(pad) = usize::try_from(query.port)
        .ok()
        .and_then(|port| input.pads.get(port))
    else {
        return 0;
    };
    if query.id == JOYPAD_ID_MASK {
        // Bit 15 (R3) lands in the sign bit: libretro reads the value back as a 16-bit mask.
        pad.bits() as i16
    } else if query.id < 16 {
        i16::from(pad.bits() & (1 << query.id) != 0)
    } else {
        0
    }
}

// Keep the id mapping in sync with `PadState` (bit n = button id n).
const _: () = assert!(JoypadButton::R3.id() == 15);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::CollectSink;
    use crate::input::ScriptedInput;
    use emu_libretro::PixelFormat;

    fn query(port: u32, device: InputDevice, id: u32) -> InputQuery {
        InputQuery {
            port,
            device,
            index: 0,
            id,
        }
    }

    #[test]
    fn input_state_mapping() {
        let mut input = InputFrame::default();
        ScriptedInput::new()
            .press(0, JoypadButton::A, 0..1)
            .press(1, JoypadButton::R3, 0..1)
            .poll(0, &mut input);
        let a = JoypadButton::A.id();
        assert_eq!(input_state(&input, query(0, InputDevice::Joypad, a)), 1);
        assert_eq!(input_state(&input, query(0, InputDevice::Joypad, 0)), 0);
        assert_eq!(
            input_state(&input, query(0, InputDevice::Joypad, JOYPAD_ID_MASK)),
            1 << 8
        );
        assert_eq!(
            input_state(&input, query(1, InputDevice::Joypad, JOYPAD_ID_MASK)) as u16,
            1 << 15
        );
        assert_eq!(input_state(&input, query(2, InputDevice::Joypad, a)), 0);
        assert_eq!(input_state(&input, query(0, InputDevice::Joypad, 16)), 0);
        assert_eq!(input_state(&input, query(0, InputDevice::Keyboard, a)), 0);
    }

    #[test]
    fn input_state_keyboard_and_mouse() {
        use crate::input::MouseButton;
        let a = RetroKey::new(97).unwrap();
        let mut input = InputFrame::default();
        ScriptedInput::new()
            .key(a, 0..1)
            .mouse(-40_000, 7, MouseButton::Right.bit(), 0..1)
            .poll(0, &mut input);
        input.mouse.wheel = -1;
        input.mouse.hwheel = 2;
        let kb = |port, id| input_state(&input, query(port, InputDevice::Keyboard, id));
        assert_eq!(kb(0, 97), 1);
        assert_eq!(kb(1, 97), 1, "single keyboard whatever the port");
        assert_eq!(kb(0, 98), 0);
        assert_eq!(kb(0, u32::from(RetroKey::LAST)), 0);
        assert_eq!(kb(0, u32::MAX), 0);
        let mouse = |port, id| input_state(&input, query(port, InputDevice::Mouse, id));
        assert_eq!(mouse(0, mouse_id::X), i16::MIN, "clamped");
        assert_eq!(mouse(0, mouse_id::Y), 7);
        assert_eq!(mouse(0, mouse_id::RIGHT), 1);
        assert_eq!(mouse(0, mouse_id::LEFT), 0);
        assert_eq!(mouse(0, mouse_id::WHEEL_DOWN), 1);
        assert_eq!(mouse(0, mouse_id::WHEEL_UP), 0);
        assert_eq!(mouse(0, mouse_id::HORIZ_WHEEL_UP), 1);
        assert_eq!(mouse(0, 99), 0);
        assert_eq!(mouse(1, mouse_id::Y), 0, "one mouse, port 0");
    }

    #[test]
    fn input_state_analog() {
        let mut input = InputFrame::default();
        let q = |port, index, id| InputQuery {
            port,
            device: InputDevice::Analog,
            index,
            id,
        };
        assert_eq!(
            input_state(&input, q(0, 0, ANALOG_ID_X)),
            0,
            "keyboard only"
        );
        input.pads[1].set_analog(AnalogStick::Left, -5, 6);
        input.pads[1].set_analog(AnalogStick::Right, i16::MIN, i16::MAX);
        input.pads[1].set(JoypadButton::L2, true);
        assert_eq!(input_state(&input, q(1, 0, ANALOG_ID_X)), -5);
        assert_eq!(input_state(&input, q(1, 0, ANALOG_ID_Y)), 6);
        assert_eq!(input_state(&input, q(1, 1, ANALOG_ID_X)), i16::MIN);
        assert_eq!(input_state(&input, q(1, 1, ANALOG_ID_Y)), i16::MAX);
        assert_eq!(input_state(&input, q(1, 0, 2)), 0, "unknown axis");
        assert_eq!(input_state(&input, q(1, 3, 0)), 0, "unknown index");
        let l2 = JoypadButton::L2.id();
        assert_eq!(input_state(&input, q(1, INDEX_ANALOG_BUTTON, l2)), i16::MAX);
        assert_eq!(input_state(&input, q(1, INDEX_ANALOG_BUTTON, 0)), 0);
        assert_eq!(input_state(&input, q(1, INDEX_ANALOG_BUTTON, 16)), 0);
        assert_eq!(
            input_state(&input, q(0, 0, ANALOG_ID_X)),
            0,
            "port 0 untouched"
        );
        assert_eq!(input_state(&input, q(2, 0, ANALOG_ID_X)), 0, "no such port");
    }

    #[test]
    fn bridge_records_polled_devices() {
        let input = InputFrame::default();
        let mut audio = CollectSink::default();
        let mut frame = FrameRgba::default();
        let mut bridge = Bridge {
            input: &input,
            audio: &mut audio,
            frame: &mut frame,
            video_updated: false,
            polled: Polled::default(),
        };
        bridge.input_state(query(0, InputDevice::Joypad, 0));
        assert!(!bridge.polled.keyboard && !bridge.polled.mouse);
        bridge.input_state(query(0, InputDevice::Mouse, mouse_id::X));
        assert!(bridge.polled.mouse && !bridge.polled.keyboard);
        bridge.input_state(query(0, InputDevice::Keyboard, 13));
        assert!(bridge.polled.keyboard);
    }

    #[test]
    fn bridge_converts_video_and_forwards_audio() {
        let input = InputFrame::default();
        let mut audio = CollectSink::default();
        let mut frame = FrameRgba::default();
        let mut bridge = Bridge {
            input: &input,
            audio: &mut audio,
            frame: &mut frame,
            video_updated: false,
            polled: Polled::default(),
        };
        bridge.video_refresh(None);
        assert!(!bridge.video_updated);
        let data = 0x00ff_0000u32.to_ne_bytes();
        bridge.video_refresh(Some(VideoFrame {
            data: &data,
            width: 1,
            height: 1,
            pitch: 4,
            format: PixelFormat::Xrgb8888,
        }));
        assert!(bridge.video_updated);
        bridge.audio_samples(&[5, 6]);
        assert_eq!(frame.pixels(), &[255, 0, 0, 255]);
        assert_eq!(audio.samples, [5, 6]);
    }

    /// Frames `0..n` of `pulses` over a backend that holds `user` during `held`: (key pressed, events).
    fn play(
        pulses: &mut KeyPulses,
        n: u64,
        user: RetroKey,
        held: std::ops::Range<u64>,
    ) -> Vec<(bool, Vec<(u16, bool)>)> {
        let mut backend = ScriptedInput::new().key(user, held);
        let mut input = InputFrame::default();
        (0..n)
            .map(|frame| {
                input.clear();
                backend.poll(frame, &mut input);
                pulses.apply(&mut input);
                (
                    input.keyboard.is_pressed(user),
                    input
                        .key_events
                        .iter()
                        .map(|e| (e.key.raw(), e.down))
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn key_pulse_holds_then_releases() {
        let f13 = RetroKey::new(294).unwrap();
        let mut pulses = KeyPulses::default();
        pulses.push(f13, crate::PULSE_FRAMES);
        let frames = play(&mut pulses, 7, f13, 0..0);
        assert_eq!(
            frames,
            [
                (true, vec![(294, true)]),
                (true, vec![]),
                (true, vec![]),
                (true, vec![]),
                (false, vec![(294, false)]),
                (false, vec![]),
                (false, vec![]),
            ]
        );
        // Fewer than 2 frames: 2.
        pulses.push(f13, 0);
        let frames = play(&mut pulses, 4, f13, 0..0);
        let held: Vec<bool> = frames.iter().map(|f| f.0).collect();
        assert_eq!(held, [true, true, false, false]);
        assert_eq!(frames[2].1, [(294, false)]);
    }

    #[test]
    fn queued_key_pulses_never_overlap() {
        let home = RetroKey::new(278).unwrap();
        let end = RetroKey::new(279).unwrap();
        let mut pulses = KeyPulses::default();
        pulses.push(home, 2);
        pulses.push(home, 2);
        pulses.push(end, 2);
        let mut input = InputFrame::default();
        let mut log = Vec::new();
        for _ in 0..10 {
            input.clear();
            pulses.apply(&mut input);
            log.push((
                input.keyboard.is_pressed(home),
                input.keyboard.is_pressed(end),
                input
                    .key_events
                    .iter()
                    .map(|e| (e.key.raw(), e.down))
                    .collect::<Vec<_>>(),
            ));
        }
        assert_eq!(
            log,
            [
                (true, false, vec![(278, true)]),
                (true, false, vec![]),
                (false, false, vec![(278, false)]),
                // The same key again: a frame released in between, so polling cores see two presses.
                (true, false, vec![(278, true)]),
                (true, false, vec![]),
                (false, false, vec![(278, false)]),
                (false, true, vec![(279, true)]),
                (false, true, vec![]),
                (false, false, vec![(279, false)]),
                (false, false, vec![]),
            ]
        );
    }

    #[test]
    fn key_pulse_never_releases_a_key_the_user_holds() {
        let a = RetroKey::new(97).unwrap();
        // Held by the user before and after the pulse: no extra event at all.
        let mut pulses = KeyPulses::default();
        pulses.push(a, 2);
        let frames = play(&mut pulses, 4, a, 0..10);
        assert!(frames.iter().all(|f| f.0));
        let events: Vec<_> = frames.iter().flat_map(|f| f.1.clone()).collect();
        assert_eq!(events, [(97, true)], "the user's own press only");
        // Pressed by the user during the pulse and still held at its end: not released by the pulse.
        let mut pulses = KeyPulses::default();
        pulses.push(a, 3);
        let frames = play(&mut pulses, 5, a, 1..10);
        assert!(frames.iter().all(|f| f.0));
        let events: Vec<_> = frames.iter().flat_map(|f| f.1.clone()).collect();
        assert_eq!(
            events,
            [(97, true), (97, true)],
            "pulse press, then the user's"
        );
    }

    #[test]
    fn key_pulse_hold_is_bounded() {
        let a = RetroKey::new(97).unwrap();
        let mut pulses = KeyPulses::default();
        pulses.push(a, u32::MAX);
        assert_eq!(pulses.queue[0].1, MAX_PULSE_FRAMES);
        pulses.push(a, 0);
        assert_eq!(pulses.queue[1].1, 2);
    }

    #[test]
    fn key_pulse_queue_is_bounded() {
        let a = RetroKey::new(97).unwrap();
        let mut pulses = KeyPulses::default();
        for _ in 0..MAX_QUEUED_PULSES + 5 {
            pulses.push(a, 2);
        }
        assert_eq!(pulses.queue.len(), MAX_QUEUED_PULSES);
    }

    fn ff(on: bool, ratio: f32) -> FastForwardOverride {
        let mut ff = FastForwardOverride::default();
        ff.fastforward = on;
        ff.ratio = ratio;
        ff
    }

    #[test]
    fn fastforward_changes_only_on_a_new_request() {
        let mut state = None;
        assert!(!update_fastforward(&mut state, None), "no request");
        assert!(update_fastforward(&mut state, Some(ff(true, 0.0))));
        assert_eq!(state, Some(ff(true, 0.0)));
        assert!(
            !update_fastforward(&mut state, Some(ff(true, 0.0))),
            "repeated"
        );
        assert!(!update_fastforward(&mut state, None), "state kept");
        assert_eq!(state, Some(ff(true, 0.0)));
        assert!(update_fastforward(&mut state, Some(ff(true, 3.0))), "ratio");
        assert!(update_fastforward(&mut state, Some(ff(false, 3.0))));
        assert_eq!(state, Some(ff(false, 3.0)));
    }
}
