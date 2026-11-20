//! Downloads (ADR 0005, ADR 0014): shared HTTP client, No-Intro DAT files, covers, cover image validation.
//! Cores and BIOS (ADR 0013) reuse `http`. Contract: `docs/design/v1.0-bibliotheque.md` §6.2.

/// Free BIOS installer (ADR 0015, `docs/design/v2.0-atari-st.md`).
pub mod bios;
/// Core manager (ADR 0013, `docs/design/v1.0-cores.md`).
pub mod cores;
mod covers;
mod dat;
mod error;
pub mod http;
mod image;
/// Background jobs polled by the UI (ADR 0013).
pub mod jobs;
mod secret;

pub use crate::covers::{
    CoverCache, CoverChain, CoverFound, CoverQuery, CoverSource, CoverSourceKind,
    DEFAULT_COVER_CACHE_BYTES, IgdbCredentials, IgdbSource, LibretroThumbnails,
    ScreenScraperCredentials, ScreenScraperSource, thumbnail_file_name,
};
pub use crate::dat::{DAT_BASE_URL, DatDownload, MAX_DAT_BYTES, dat_url, download_dat};
pub use crate::error::AssetError;
pub use crate::image::{
    CoverImage, ImageFormat, MAX_COVER_BYTES, MAX_COVER_SIDE, decode_cover, sniff_format,
};
pub use crate::secret::Secret;
