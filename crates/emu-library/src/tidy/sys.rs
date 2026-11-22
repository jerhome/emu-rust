//! The file system calls that change something (design v7.1 §3.4 « une couche injectable »): the execution, the
//! undo and the recovery go through [`Sys`] only, so the tests can make any call fail or « crash » the process
//! after any of them (FV0 15). Read-only calls (`symlink_metadata`, `read_dir`, reading for a CRC) are direct.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

pub(crate) trait Sys {
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
    /// Fails when `to` exists: never overwrites.
    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Makes the entries of a folder durable (Unix: `fsync` of the folder; no-op elsewhere).
    fn sync_dir(&self, path: &Path) -> io::Result<()>;
    /// Writes `bytes` at the current position, then `sync_all`.
    fn write_synced(&self, file: &mut File, bytes: &[u8]) -> io::Result<()>;
    /// Cuts `file` back to `len` bytes, then `sync_all` (a torn journal line, before appending).
    fn truncate_synced(&self, file: &File, len: u64) -> io::Result<()>;
    /// Around the database transaction: lets the tests stop just before or just after it.
    fn checkpoint(&self) -> io::Result<()>;
}

/// The real file system.
pub(crate) struct RealSys;

impl Sys for RealSys {
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        fs::create_dir(path)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir(path)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    #[cfg(unix)]
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        File::open(path)?.sync_all()
    }

    #[cfg(not(unix))]
    fn sync_dir(&self, _path: &Path) -> io::Result<()> {
        // Windows cannot open a folder without backup semantics; NTFS journals its metadata.
        Ok(())
    }

    fn write_synced(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn truncate_synced(&self, file: &File, len: u64) -> io::Result<()> {
        file.set_len(len)?;
        file.sync_all()
    }

    fn checkpoint(&self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod faulty {
    //! Fault injection: call number `at` (0-based) fails; in `Crash` mode every later call fails too (the process
    //! is gone: nothing more reaches the disk), and a failing journal write leaves half of its bytes (torn line).

    use std::cell::Cell;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Mode {
        /// Only call `at` fails, then everything works again (in-run rollback).
        Once,
        /// Call `at` and every later one fail (crash), the journal write torn.
        Crash,
        /// Call `at` is fully carried out (journal write synced) but reports an error; later calls work.
        DoneButFailed,
    }

    pub(crate) struct Faulty {
        pub(crate) at: u64,
        pub(crate) mode: Mode,
        calls: Cell<u64>,
    }

    impl Faulty {
        pub(crate) fn new(at: u64, mode: Mode) -> Self {
            Faulty {
                at,
                mode,
                calls: Cell::new(0),
            }
        }

        /// Calls made (or attempted) so far.
        pub(crate) fn calls(&self) -> u64 {
            self.calls.get()
        }

        /// `true` when this call must fail.
        fn fails(&self) -> bool {
            let n = self.calls.get();
            self.calls.set(n + 1);
            match self.mode {
                Mode::Once | Mode::DoneButFailed => n == self.at,
                Mode::Crash => n >= self.at,
            }
        }

        fn injected() -> io::Error {
            io::Error::other("panne injectée")
        }
    }

    impl Sys for Faulty {
        fn create_dir(&self, path: &Path) -> io::Result<()> {
            if self.fails() {
                if self.mode == Mode::DoneButFailed {
                    RealSys.create_dir(path)?;
                }
                return Err(Self::injected());
            }
            RealSys.create_dir(path)
        }

        fn remove_dir(&self, path: &Path) -> io::Result<()> {
            if self.fails() {
                if self.mode == Mode::DoneButFailed {
                    RealSys.remove_dir(path)?;
                }
                return Err(Self::injected());
            }
            RealSys.remove_dir(path)
        }

        fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
            if self.fails() {
                if self.mode == Mode::DoneButFailed {
                    RealSys.hard_link(from, to)?;
                }
                return Err(Self::injected());
            }
            RealSys.hard_link(from, to)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            if self.fails() {
                if self.mode == Mode::DoneButFailed {
                    RealSys.remove_file(path)?;
                }
                return Err(Self::injected());
            }
            RealSys.remove_file(path)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if self.fails() {
                if self.mode == Mode::DoneButFailed {
                    RealSys.rename(from, to)?;
                }
                return Err(Self::injected());
            }
            RealSys.rename(from, to)
        }

        // No real `fsync` in `sync_dir` / `write_synced`: nothing here survives a power loss, thousands of syncs
        // only make the tests slow; the order of the calls is what they check.
        fn sync_dir(&self, _path: &Path) -> io::Result<()> {
            if self.fails() {
                return Err(Self::injected());
            }
            Ok(())
        }

        fn write_synced(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
            if self.fails() {
                if self.mode == Mode::DoneButFailed {
                    file.write_all(bytes)?;
                }
                if self.mode == Mode::Crash && self.calls.get() == self.at + 1 {
                    // The crash cut the write: half of it reached the disk.
                    file.write_all(&bytes[..bytes.len() / 2])?;
                }
                return Err(Self::injected());
            }
            file.write_all(bytes)
        }

        fn truncate_synced(&self, file: &File, len: u64) -> io::Result<()> {
            if self.fails() {
                if self.mode == Mode::DoneButFailed {
                    RealSys.truncate_synced(file, len)?;
                }
                return Err(Self::injected());
            }
            RealSys.truncate_synced(file, len)
        }

        fn checkpoint(&self) -> io::Result<()> {
            if self.fails() {
                return Err(Self::injected());
            }
            Ok(())
        }
    }
}
