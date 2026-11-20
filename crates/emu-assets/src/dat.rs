//! No-Intro and TOSEC DAT files from libretro-database (ADR 0014 §2, ADR 0015 §1): downloaded at run time,
//! never shipped. The caller picks the base from `SystemInfo::dat` (both are clrmamepro DAT files).

use std::io::Write;
use std::path::Path;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::AssetError;
use crate::http::{HttpClient, Method, Request, sync_dir};

pub const DAT_BASE_URL: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/";
/// TOSEC DAT files (`Atari - ST`: floppy images `.st` / `.stx` / `.ipf` / `.dim`).
pub const DAT_TOSEC_BASE_URL: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/tosec/";
/// Same bound as `emu_library::MAX_DAT_SIZE` (the crates do not depend on each other).
pub const MAX_DAT_BYTES: u64 = 64 * 1024 * 1024;

/// One URL path segment: everything but RFC 3986 unreserved characters is encoded (`/`, `?`, `#` included).
pub(crate) const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// `<base>/<dat name, percent-encoded>.dat`.
pub fn dat_url(base: &str, dat_name: &str) -> String {
    let separator = if base.ends_with('/') { "" } else { "/" };
    format!(
        "{base}{separator}{}.dat",
        utf8_percent_encode(dat_name, PATH_SEGMENT)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatDownload {
    Updated {
        etag: Option<String>,
        bytes: u64,
    },
    /// 304 for `If-None-Match: etag`: `dest` untouched.
    NotModified,
}

/// Conditional GET (`If-None-Match`), atomic write of `dest` (parent created), bounded by [`MAX_DAT_BYTES`].
/// The body is held in memory (≤ 64 MiB, a few MiB in practice) before the write. Blocking: worker thread only.
///
/// `etag` must be the one returned when `dest` was written: callers pass it only if `dest` still exists. As a
/// safeguard it is ignored when `dest` is missing (a 304 would otherwise leave the caller without a file).
pub fn download_dat(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    etag: Option<&str>,
) -> Result<DatDownload, AssetError> {
    let etag = etag.filter(|_| dest.is_file());
    let mut headers = Vec::new();
    if let Some(etag) = etag.filter(|e| !e.is_empty()) {
        headers.push(("If-None-Match", etag.to_owned()));
    }
    let response = client.send(&Request {
        method: Method::Get,
        url,
        headers,
        body: None,
        max_bytes: MAX_DAT_BYTES,
    })?;
    match response.status {
        200 => {}
        304 if etag.is_some() => return Ok(DatDownload::NotModified),
        429 => {
            return Err(AssetError::Quota {
                retry_after: response.retry_after,
            });
        }
        status => return Err(AssetError::Http { status }),
    }
    write_atomic(dest, &response.body)?;
    Ok(DatDownload::Updated {
        etag: response.etag,
        bytes: response.body.len() as u64,
    })
}

/// Temporary file next to `dest`, synced, then renamed; on error `dest` is untouched and the temporary removed.
pub(crate) fn write_atomic(dest: &Path, data: &[u8]) -> Result<(), AssetError> {
    let io_err = |source| AssetError::Io {
        path: dest.to_path_buf(),
        source,
    };
    let parent = match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let file_name = dest
        .file_name()
        .ok_or_else(|| io_err(std::io::Error::from(std::io::ErrorKind::InvalidInput)))?;
    std::fs::create_dir_all(parent).map_err(io_err)?;
    let mut temp = tempfile::Builder::new()
        .prefix(&format!(".{}.tmp-", file_name.to_string_lossy()))
        .tempfile_in(parent)
        .map_err(io_err)?;
    temp.write_all(data).map_err(io_err)?;
    temp.as_file().sync_all().map_err(io_err)?;
    temp.persist(dest).map_err(|e| io_err(e.error))?;
    sync_dir(parent);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encodes_the_name() {
        assert_eq!(
            dat_url(DAT_BASE_URL, "Nintendo - Game Boy"),
            format!("{DAT_BASE_URL}Nintendo%20-%20Game%20Boy.dat")
        );
        assert_eq!(
            dat_url("https://x.test/d", "Sega - Mega Drive - Genesis"),
            "https://x.test/d/Sega%20-%20Mega%20Drive%20-%20Genesis.dat"
        );
        assert_eq!(
            dat_url("https://x.test/", "a/../b?c#d"),
            "https://x.test/a%2F..%2Fb%3Fc%23d.dat"
        );
        assert_eq!(
            dat_url(DAT_TOSEC_BASE_URL, "Atari - ST"),
            "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/tosec/Atari%20-%20ST.dat"
        );
    }
}
