//! Disk control (`RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE` 13, `..._EXT_INTERFACE` 58,
//! `GET_DISK_CONTROL_INTERFACE_VERSION` 57 answered with 1). ADR 0015.
//!
//! Every call goes to a function pointer the core registered through the environment; it happens on the core thread
//! (`Core` is `!Send`) and outside `retro_run` (`DiskControl` borrows the `Core` mutably), after the pointers were
//! copied out of the environment state (the core may re-enter the environment, e.g. with `SET_MESSAGE`).

use std::ffi::{CStr, c_char, c_uint};
use std::path::{Path, PathBuf};

use crate::core::Core;
use crate::error::LibretroError;
use crate::{callbacks, sys};

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
///
/// Without loaded content, getters answer neutrally (`false`, `0`, `None`) and the other operations return
/// [`LibretroError::NoGame`], except [`DiskControl::set_initial_image`], which is only valid before loading.
/// A callback the core left NULL gives [`LibretroError::DiskControlUnsupported`].
pub struct DiskControl<'a> {
    pub(crate) core: &'a mut Core,
    pub(crate) kind: DiskControlKind,
    /// Copied from the environment state; the basic interface leaves the extended entries `None`.
    pub(crate) fns: sys::retro_disk_control_ext_callback,
}

/// Size of the buffer handed to `get_image_path` / `get_image_label` (`PATH_MAX` on Linux, as RetroArch uses).
const TEXT_BUFFER_LEN: usize = 4096;

impl DiskControl<'_> {
    pub fn kind(&self) -> DiskControlKind {
        self.kind
    }

    /// `f` if content is loaded, the core provided it, else the matching error.
    fn require<T>(&self, f: Option<T>) -> Result<T, LibretroError> {
        if !self.core.game_loaded() {
            return Err(LibretroError::NoGame);
        }
        f.ok_or(LibretroError::DiskControlUnsupported)
    }

    /// `f` if content is loaded and the core provided it (neutral getters).
    fn query<T>(&self, f: Option<T>) -> Option<T> {
        f.filter(|_| self.core.game_loaded())
    }

    pub fn is_ejected(&mut self) -> bool {
        let Some(f) = self.query(self.fns.get_eject_state) else {
            return false;
        };
        // SAFETY: registered by this core, whose library is loaded while `self.core` lives; owning thread, outside
        // `retro_run`, content loaded; no arguments. The result is a `u8` (any byte is valid; a C `bool` from a
        // buggy core could be neither 0 nor 1) and is tested with `!= 0`.
        unsafe { f() != 0 }
    }

    pub fn set_ejected(&mut self, ejected: bool) -> Result<(), LibretroError> {
        let f = self.require(self.fns.set_eject_state)?;
        // SAFETY: as in `is_ejected`; plain `bool` argument, `u8` result.
        check(unsafe { f(ejected) } != 0, "set_eject_state")
    }

    pub fn image_index(&mut self) -> u32 {
        let Some(f) = self.query(self.fns.get_image_index) else {
            return 0;
        };
        // SAFETY: as in `is_ejected`.
        unsafe { f() }
    }

    pub fn set_image_index(&mut self, index: u32) -> Result<(), LibretroError> {
        let f = self.require(self.fns.set_image_index)?;
        // SAFETY: as in `is_ejected`; libretro.h makes the core validate `index` (it returns false).
        check(unsafe { f(c_uint::from(index)) } != 0, "set_image_index")
    }

    pub fn num_images(&mut self) -> u32 {
        let Some(f) = self.query(self.fns.get_num_images) else {
            return 0;
        };
        // SAFETY: as in `is_ejected`.
        unsafe { f() }
    }

    /// `None` removes the image at `index` (later indices shift down, per libretro.h).
    /// The path is passed to the core as `retro_game_info.path` with `data = NULL`, `size = 0` (computer cores set
    /// `need_fullpath`). The C string is kept alive until the content is unloaded, in case the core keeps the
    /// pointer. Non-UTF-8 paths: raw bytes on Unix, lossy UTF-8 on Windows (logged, see `path_to_cstring`).
    /// A path containing a NUL byte gives [`LibretroError::Io`] (`InvalidInput`) without calling the core.
    pub fn replace_image(&mut self, index: u32, image: Option<&Path>) -> Result<(), LibretroError> {
        let f = self.require(self.fns.replace_image_index)?;
        let ok = match image {
            None => {
                // SAFETY: as in `is_ejected`; libretro.h documents a NULL `info` as "remove this image".
                unsafe { f(c_uint::from(index), std::ptr::null()) != 0 }
            }
            Some(path) => {
                let path = self.core.retain_disk_path(disk_cstring(path)?);
                let info = sys::retro_game_info {
                    path,
                    data: std::ptr::null(),
                    size: 0,
                    meta: std::ptr::null(),
                };
                // SAFETY: as in `is_ejected`. `info` lives on this stack frame for the call; its `path` is a
                // NUL-terminated string kept by `Core` until the content is unloaded; `data` is NULL with size 0.
                unsafe { f(c_uint::from(index), &raw const info) != 0 }
            }
        };
        check(ok, "replace_image_index")
    }

    /// Appends an empty slot; fill it with [`DiskControl::replace_image`].
    pub fn add_image(&mut self) -> Result<(), LibretroError> {
        let f = self.require(self.fns.add_image_index)?;
        // SAFETY: as in `is_ejected`.
        check(unsafe { f() } != 0, "add_image_index")
    }

    /// Extended only, and only before `load_game` (restores the disk inserted at the previous session):
    /// [`LibretroError::GameAlreadyLoaded`] afterwards. The core checks `index` / `path` itself during
    /// `retro_load_game` and falls back to image 0 on mismatch.
    pub fn set_initial_image(&mut self, index: u32, path: &Path) -> Result<(), LibretroError> {
        if self.core.game_loaded() {
            return Err(LibretroError::GameAlreadyLoaded);
        }
        let f = self
            .fns
            .set_initial_image
            .ok_or(LibretroError::DiskControlUnsupported)?;
        // libretro.h: the core reads the path during the following `retro_load_game`, so it must outlive this call.
        let path = self.core.retain_disk_path(disk_cstring(path)?);
        // SAFETY: registered by this core, whose library is loaded; owning thread; libretro.h asks for this call
        // before `retro_load_game` (checked above). `path` is NUL-terminated and kept by `Core`.
        check(
            unsafe { f(c_uint::from(index), path) } != 0,
            "set_initial_image",
        )
    }

    /// Extended only; `None` otherwise or when the core has no path.
    pub fn image_path(&mut self, index: u32) -> Option<PathBuf> {
        let f = self.query(self.fns.get_image_path)?;
        let bytes = read_text(|buf, len| {
            // SAFETY: as in `is_ejected`; `buf` is writable for `len` bytes during the call (`read_text`) and the
            // core writes at most `len` bytes (libretro.h contract of `s` / `len`).
            unsafe { f(c_uint::from(index), buf, len) != 0 }
        })?;
        Some(path_from_bytes(bytes))
    }

    /// Extended only; `None` otherwise or when the core has no label. Invalid UTF-8 is replaced (lossy).
    pub fn image_label(&mut self, index: u32) -> Option<String> {
        let f = self.query(self.fns.get_image_label)?;
        let bytes = read_text(|buf, len| {
            // SAFETY: as in `image_path`.
            unsafe { f(c_uint::from(index), buf, len) != 0 }
        })?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn check(ok: bool, operation: &'static str) -> Result<(), LibretroError> {
    if ok {
        Ok(())
    } else {
        Err(LibretroError::DiskControlFailed(operation))
    }
}

fn disk_cstring(path: &Path) -> Result<std::ffi::CString, LibretroError> {
    callbacks::path_to_cstring(path, "disk image path").ok_or_else(|| LibretroError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains a NUL byte"),
    })
}

/// Lets `fill` write a C string into a zeroed buffer of `len` = `TEXT_BUFFER_LEN` bytes, valid for the call only,
/// and copies it out. `None` if `fill` returns false, the string is empty, or no terminating NUL was written.
fn read_text(fill: impl FnOnce(*mut c_char, usize) -> bool) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; TEXT_BUFFER_LEN];
    if !fill(buf.as_mut_ptr().cast(), buf.len()) {
        return None;
    }
    let text = CStr::from_bytes_until_nul(&buf).ok()?.to_bytes();
    (!text.is_empty()).then(|| text.to_vec())
}

fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }
    // libretro uses UTF-8 paths on Windows.
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
    }
}

impl Core {
    /// `None` when the core registered no disk control interface. Available before `load_game` (for
    /// [`DiskControl::set_initial_image`]).
    pub fn disk_control(&mut self) -> Option<DiskControl<'_>> {
        let interface = callbacks::disk_interface()?;
        Some(DiskControl {
            core: self,
            kind: interface.kind,
            fns: interface.fns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_text_copies_until_nul() {
        let text = read_text(|buf, len| {
            assert_eq!(len, TEXT_BUFFER_LEN);
            // SAFETY: `buf` is writable for `len` >= 6 bytes.
            unsafe { std::ptr::copy_nonoverlapping(c"Disk 2".as_ptr(), buf, 6) };
            true
        });
        assert_eq!(text.as_deref(), Some(&b"Disk 2"[..]));
    }

    #[test]
    fn read_text_rejects_failure_empty_and_unterminated() {
        assert_eq!(read_text(|_, _| false), None);
        assert_eq!(read_text(|_, _| true), None);
        let unterminated = read_text(|buf, len| {
            // SAFETY: `buf` is writable for `len` bytes.
            unsafe { std::ptr::write_bytes(buf, b'x', len) };
            true
        });
        assert_eq!(unterminated, None);
    }

    #[test]
    fn check_maps_false_to_operation_error() {
        assert!(check(true, "x").is_ok());
        assert!(matches!(
            check(false, "set_eject_state"),
            Err(LibretroError::DiskControlFailed("set_eject_state"))
        ));
    }

    #[test]
    fn nul_in_path_is_rejected_before_the_core() {
        assert!(matches!(
            disk_cstring(Path::new("a\0b.st")),
            Err(LibretroError::Io { .. })
        ));
        assert_eq!(
            disk_cstring(Path::new("/d/Game (Disk 1 of 2).st"))
                .unwrap()
                .as_bytes(),
            b"/d/Game (Disk 1 of 2).st"
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_round_trip_on_unix() {
        let bytes = b"/d/\xe9t\xe9.st".to_vec();
        let path = path_from_bytes(bytes.clone());
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(path.as_os_str().as_bytes(), &bytes[..]);
        assert_eq!(disk_cstring(&path).unwrap().as_bytes(), &bytes[..]);
    }
}
