//! Core manager (ADR 0013, docs/design/v1.0-cores.md): buildbot index and `.info` files, install / update /
//! remove / rollback under `<data>/cores/<os>-<arch>/`, integrity, core choice per system, firmware checks.

mod archive;
mod firmware;
mod id;
mod index;
mod info;
mod manager;
mod platform;
mod select;

pub use archive::{ArchiveError, ExtractLimits, extract_core, read_info_bundle};
pub use firmware::{
    FirmwareCheck, FirmwareError, FirmwareState, FirmwareStatus, KnownFirmware, MAX_FIRMWARE_SIZE,
    check_core_firmware, check_firmware, check_required,
};
pub use id::CoreId;
pub use index::{BuildDate, MAX_INDEX_ENTRIES, RemoteCore, parse_index_extended};
pub(crate) use info::is_safe_relative_path;
pub use info::{CoreInfo, FirmwareSpec, InfoError, MAX_INFO_SIZE, parse_info};
pub use manager::{
    BuildbotSource, CATALOG_MAX_AGE, CoreDirs, CoreError, CoreManager, CoreUpdate,
    DOWNLOAD_ATTEMPTS, InstalledCore, MAX_CORE_ZIP_BYTES, MAX_INDEX_BYTES, MAX_INFO_BUNDLE_BYTES,
    Origin, RemoteCatalog,
};
pub use platform::Platform;
pub use select::{
    BuiltinFirmware, CoreActions, CoreChoice, CoreMediaPolicy, CorePreferences, FirmwareFallback,
    FirmwareVariant, FolderPolicy, LayoutOption, M3uLaunchPref, PadDevices, RequiredFirmware,
    SaveScopePref, choose_core,
};
