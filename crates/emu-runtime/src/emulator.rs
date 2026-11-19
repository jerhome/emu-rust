use std::path::{Path, PathBuf};
use std::time::SystemTime;

use emu_libretro::{
    AvInfo, Core, CoreConfig, CoreMessage, CoreOption, CoreOptionCategory, FrontendHandler,
    InputDevice, InputQuery, JOYPAD_ID_MASK, JoypadButton, MemoryKind, SystemInfo, VideoFrame,
};

use crate::audio::AudioSink;
use crate::error::RuntimeError;
use crate::frame::{FrameRgba, convert_frame};
use crate::input::{InputBackend, InputFrame};
use crate::saves::StateHeader;

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
}

/// Headless emulation session: one core, one game. Same threading rules as `emu_libretro::Core` (!Send).
pub struct Emulator {
    core: Core,
    /// Cached copy so `av_info()` is infallible; refreshed when the core reports a change.
    av_info: AvInfo,
    frame: FrameRgba,
    input: InputFrame,
    frame_count: u64,
}

impl Emulator {
    /// Loads the core then the game. Fails with `AlreadyLoaded` if another session is alive in the process.
    pub fn load(core: &Path, rom: &Path, options: EmulatorOptions) -> Result<Self, RuntimeError> {
        let config = CoreConfig {
            system_dir: options.system_dir,
            save_dir: options.save_dir,
            assets_dir: options.assets_dir,
            variables: options.core_variables,
        };
        let mut core = Core::load(core, config)?;
        let av_info = core.load_game(rom)?;
        let info = core.system_info();
        tracing::info!(
            core = %info.library_name,
            version = %info.library_version,
            width = av_info.geometry.base_width,
            height = av_info.geometry.base_height,
            fps = av_info.timing.fps,
            sample_rate = av_info.timing.sample_rate,
            "game loaded"
        );
        Ok(Self {
            core,
            av_info,
            frame: FrameRgba::default(),
            input: InputFrame::default(),
            frame_count: 0,
        })
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
        // input without polling, and the backend sees exactly one call per frame.
        self.input = InputFrame::default();
        input.poll(self.frame_count, &mut self.input);

        let mut bridge = Bridge {
            input: &self.input,
            audio,
            frame: &mut self.frame,
            video_updated: false,
        };
        let events = self.core.run(&mut bridge)?;
        let video_updated = bridge.video_updated;
        self.frame_count += 1;

        if events.av_info_changed
            && let Some(av_info) = self.core.av_info()
        {
            self.av_info = *av_info;
        }
        Ok(FrameStatus {
            video_updated,
            av_info_changed: events.av_info_changed,
            shutdown_requested: events.shutdown_requested,
            options_changed: events.options_changed,
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
        todo!()
    }

    /// `game_name_from_path` of the content path given to `load`.
    pub fn game_name(&self) -> &str {
        todo!()
    }

    /// Where SRAM is persisted (whether or not the core exposes any).
    pub fn sram_path(&self) -> PathBuf {
        todo!()
    }

    /// Path of savestate `slot`; `InvalidSlot` if `slot >= STATE_SLOTS`.
    pub fn state_path(&self, slot: u8) -> Result<PathBuf, RuntimeError> {
        todo!("{slot}")
    }

    /// Writes SRAM with `write_atomic` if it differs from what was last loaded or written. Returns `true` if
    /// a file was written; `Ok(false)` when unchanged or the core exposes no SRAM.
    pub fn flush_sram(&mut self) -> Result<bool, RuntimeError> {
        todo!()
    }

    /// Core memory region (tests, future cheats / achievements).
    pub fn memory(&self, kind: MemoryKind) -> Option<&[u8]> {
        todo!("{kind:?}")
    }

    /// Raw core serialization (no container): in-memory snapshots for tests and future rewind.
    pub fn serialize_state(&mut self) -> Result<Vec<u8>, RuntimeError> {
        todo!()
    }

    pub fn unserialize_state(&mut self, data: &[u8]) -> Result<(), RuntimeError> {
        todo!("{}", data.len())
    }

    /// `encode_state` + `write_atomic` to `state_path(slot)`; returns the path.
    pub fn save_state(&mut self, slot: u8) -> Result<PathBuf, RuntimeError> {
        todo!("{slot}")
    }

    /// Reads, `decode_state`s (`WrongCore` if another core id), warns on core version mismatch, then
    /// unserializes. On any error the running game is left untouched.
    pub fn load_state(&mut self, slot: u8) -> Result<(), RuntimeError> {
        todo!("{slot}")
    }

    /// Header of slot `slot` and file modification time, for the menu; `None` if absent or unreadable.
    pub fn state_info(&self, slot: u8) -> Option<(StateHeader, SystemTime)> {
        todo!("{slot}")
    }

    pub fn options(&mut self) -> Vec<CoreOption> {
        todo!()
    }

    pub fn option_categories(&self) -> Vec<CoreOptionCategory> {
        todo!()
    }

    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), RuntimeError> {
        todo!("{key}={value}")
    }

    /// Forwarded to the core (`GET_FASTFORWARDING`).
    pub fn set_fast_forwarding(&mut self, enabled: bool) {
        todo!("{enabled}")
    }

    /// OSD messages from the core since the last call.
    pub fn take_messages(&mut self) -> Vec<CoreMessage> {
        todo!()
    }
}

/// Per-`run_frame` callback target; borrows everything so nothing is allocated.
struct Bridge<'a> {
    input: &'a InputFrame,
    audio: &'a mut dyn AudioSink,
    frame: &'a mut FrameRgba,
    video_updated: bool,
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
        input_state(self.input, query)
    }
}

fn input_state(input: &InputFrame, query: InputQuery) -> i16 {
    if query.device != InputDevice::Joypad {
        return 0;
    }
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
    fn bridge_converts_video_and_forwards_audio() {
        let input = InputFrame::default();
        let mut audio = CollectSink::default();
        let mut frame = FrameRgba::default();
        let mut bridge = Bridge {
            input: &input,
            audio: &mut audio,
            frame: &mut frame,
            video_updated: false,
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
}
