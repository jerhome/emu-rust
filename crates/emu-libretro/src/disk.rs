//! Disk control (`RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE` 13, `..._EXT_INTERFACE` 58,
//! `GET_DISK_CONTROL_INTERFACE_VERSION` 57 answered with 1). ADR 0015.
//!
//! Contract only (version 2.0): bodies are filled by the `libretro-ffi` agent.

use std::path::{Path, PathBuf};

use crate::core::Core;
use crate::error::LibretroError;

/// Which interface the core registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskControlKind {
    /// `retro_disk_control_callback` (13): no initial image, paths or labels.
    Basic,
    /// `retro_disk_control_ext_callback` (58).
    Extended,
}

/// Safe view over the core's disk control callbacks, borrowed from [`Core::disk_control`].
///
/// Indices are 0-based; `image_index() == num_images()` means "no disk". libretro requires the tray to be
/// ejected around `set_image_index` / `replace_image`: these methods return
/// [`LibretroError::DiskControlFailed`] when the core refuses, they never eject on their own
/// (the caller, `emu-runtime`, sequences eject → change → insert).
pub struct DiskControl<'a> {
    // Read once the stubs are implemented (libretro-ffi, version 2.0).
    #[allow(dead_code)]
    pub(crate) core: &'a mut Core,
    pub(crate) kind: DiskControlKind,
}

impl DiskControl<'_> {
    pub fn kind(&self) -> DiskControlKind {
        self.kind
    }

    pub fn is_ejected(&mut self) -> bool {
        todo!("libretro-ffi: get_eject_state")
    }

    pub fn set_ejected(&mut self, ejected: bool) -> Result<(), LibretroError> {
        let _ = ejected;
        todo!("libretro-ffi: set_eject_state")
    }

    pub fn image_index(&mut self) -> u32 {
        todo!("libretro-ffi: get_image_index")
    }

    pub fn set_image_index(&mut self, index: u32) -> Result<(), LibretroError> {
        let _ = index;
        todo!("libretro-ffi: set_image_index")
    }

    pub fn num_images(&mut self) -> u32 {
        todo!("libretro-ffi: get_num_images")
    }

    /// `None` removes the image at `index` (later indices shift down, per libretro.h).
    /// The path is passed to the core as `retro_game_info.path` with `data = NULL` (computer cores set
    /// `need_fullpath`); it must outlive nothing: the core copies it.
    pub fn replace_image(&mut self, index: u32, image: Option<&Path>) -> Result<(), LibretroError> {
        let _ = (index, image);
        todo!("libretro-ffi: replace_image_index")
    }

    /// Appends an empty slot; fill it with [`DiskControl::replace_image`].
    pub fn add_image(&mut self) -> Result<(), LibretroError> {
        todo!("libretro-ffi: add_image_index")
    }

    /// Extended only, and only before `load_game` (restores the disk inserted at the previous session).
    pub fn set_initial_image(&mut self, index: u32, path: &Path) -> Result<(), LibretroError> {
        let _ = (index, path);
        todo!("libretro-ffi: set_initial_image")
    }

    /// Extended only; `None` otherwise or when the core has no path.
    pub fn image_path(&mut self, index: u32) -> Option<PathBuf> {
        let _ = index;
        todo!("libretro-ffi: get_image_path")
    }

    /// Extended only; `None` otherwise or when the core has no label.
    pub fn image_label(&mut self, index: u32) -> Option<String> {
        let _ = index;
        todo!("libretro-ffi: get_image_label")
    }
}

impl Core {
    /// `None` when the core registered no disk control interface.
    pub fn disk_control(&mut self) -> Option<DiskControl<'_>> {
        todo!(
            "libretro-ffi: return DiskControl when the core called SET_DISK_CONTROL(_EXT)_INTERFACE"
        )
    }
}
