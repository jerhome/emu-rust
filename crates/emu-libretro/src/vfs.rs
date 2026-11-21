//! `retro_vfs_interface` v3 over `std::fs`, answered to `GET_VFS_INTERFACE` (45) only when
//! `CoreConfig::provide_vfs` is set (ADR 0018, design v4.0 §3.1). Filled by DF1.
//!
//! Contract for DF1:
//! - 19 `unsafe extern "C"` functions in a `static` table: `get_path`, `open`, `close`, `size`, `tell`, `seek`,
//!   `read`, `write`, `flush`, `remove`, `rename`, `truncate`, `stat`, `mkdir`, `opendir`, `readdir`,
//!   `dirent_get_name`, `dirent_is_dir`, `closedir`.
//! - `required_interface_version > 3` ⇒ the environment call answers `false`; otherwise version 3 is written.
//! - No thread-local state and no `with_env`: the core may call these from its own threads. Handles are
//!   `Box<VfsFile { file, path: CString }>` / `Box<VfsDir { iter, current: Option<(CString, bool)> }>`, freed by
//!   `close` / `closedir`.
//! - Paths must be UTF-8 (else `NULL` / `-1`); no `panic` crosses the boundary (`catch_unwind` ⇒ error value);
//!   access modes and `stat` flags as in `libretro.h`; `mkdir`: 0, -2 if it exists, -1 otherwise;
//!   `dirent_get_name` valid until the next `readdir`; `opendir(include_hidden = false)` skips names starting
//!   with `.`.

/// Interface version written back to the core.
#[allow(dead_code)] // DF1 wires the table into `callbacks.rs`.
pub(crate) const VFS_INTERFACE_VERSION: u32 = 3;
