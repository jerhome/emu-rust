//! Emulation loop, software-frame conversion, audio sinks, input backends, pacing and GL presentation.
#![allow(unsafe_code)]

mod added;
mod audio;
mod audio_out;
#[cfg(any(feature = "audio-cpal", test))]
mod audio_reopen;
mod audio_ring;
mod content;
mod dialog;
mod diskorder;
mod dry_run;
mod emulator;
mod error;
mod frame;
mod fsname;
mod gl;
mod identity;
mod input;
mod media;
mod pacing;
mod resample;
mod saves;
mod snapshot;
mod timing;
mod workcopy;

pub use emu_libretro::{
    AvInfo, ControllerType, CoreMessage, CoreOption, CoreOptionCategory, CoreOptionValue,
    DiskControlKind, FastForwardOverride, FrameKind, GameGeometry, HwContextKind, HwRenderRequest,
    InputDescriptor, JoypadButton, KeyEvent, KeyModifiers, LibretroError, LogLevel,
    MAX_INPUT_DESCRIPTORS, MemoryKind, RetroKey, SystemInfo, SystemTiming, mouse_id,
};

pub use crate::added::{AddedDisk, OrphanCopy, OrphanKind};
pub use crate::added::{orphan_copies, remove_orphan_copies};
pub use crate::audio::{AudioSink, CollectSink, NullSink, write_wav};
#[cfg(feature = "audio-cpal")]
pub use crate::audio_out::CpalSink;
pub use crate::audio_out::{AudioOutput, AudioStatus};
pub use crate::audio_ring::{
    AudioConfig, AudioError, AudioStats, OutputSample, RingReader, RingSink, audio_ring,
};
pub use crate::content::archive_entry_names;
pub use crate::dialog::fatal_dialog;
pub use crate::dry_run::{
    DryRunOutcome, DryRunReport, DryRunRequest, dry_run, suppress_crash_dialogs,
};
pub use crate::emulator::{Emulator, EmulatorOptions, FrameStatus, MAX_CONTENT_SIZE};
pub use crate::error::RuntimeError;
pub use crate::frame::{FrameRgba, convert_frame};
pub use crate::gl::{GlWindow, Scaling};
pub use crate::identity::{GameIdentity, IdentityNotice, IdentityRequest, SaveScope};
pub use crate::identity::{RelocateReport, relocate_identities};
pub use crate::identity::{identity_request_for, resolve_game_identity};
pub use crate::input::{
    AnalogStick, InputBackend, InputFrame, KeyboardState, MAX_PORTS, MouseButton, MouseState,
    NullInput, PULSE_FRAMES, PadState, ScriptedInput,
};
pub use crate::media::{
    ContentSpec, DiskSlot, Disks, M3uLaunch, MAX_COPY_BYTES, MAX_MEDIA_IMAGES, MediaPolicy,
    MediaRules, MediaSet, MultiDisc, NO_GAME_NAME, PreparedMedia, WorkDirNotice, prepare_media,
    reset_core_game_files, reset_work_dir, reset_working_copies,
};
pub use crate::pacing::{
    AudioLevel, DEFAULT_FAST_FORWARD_RATIO, MAX_BURST, PaceDecision, Pacer, Speed, SyncMode,
};
pub use crate::resample::{MAX_RATE_ADJUST, StereoResampler, rate_adjust};
pub use crate::saves::StateOrigin;
pub use crate::saves::adopt_family_save;
pub use crate::saves::{
    FileMode, STATE_FORMAT_VERSION, STATE_MAGIC, STATE_SLOTS, StateFileError, StateHeader,
    core_id_from_path, decode_state, encode_state, game_name_from_path, sram_path, state_path,
    write_atomic,
};
pub use crate::snapshot::{
    SnapshotLimits, SnapshotReport, SnapshotStatus, snapshot_folder, snapshot_path, snapshot_status,
};
pub use crate::timing::FramePacer;
