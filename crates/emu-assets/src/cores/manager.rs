//! Installed cores and their lifecycle. `CoreManager` is `Send + Sync` and used through an `Arc`: quick
//! queries run on the UI thread, downloads in [`crate::jobs::Job`]s. Internal state sits behind a mutex
//! held only briefly (never during a download), so the UI never waits on the network.
//!
//! Several processes may share `<cores>` (UI + CLI, two containers on the same `/data`). Every section that
//! renames core files or writes `manifest.toml` holds an exclusive lock on `<cores>/.lock` (a few file system
//! calls, never a download), re-reads the manifest from disk and applies its change to that copy, so entries
//! written by another process are kept. Order: directory lock first, then the state mutex.
//!
//! Layout (ADR 0006 / 0013), `<cores>` = `<data>/cores/<os>-<arch>/`:
//! ```text
//! <cores>/<id>_libretro.<ext>        installed core
//! <cores>/<id>_libretro.<ext>.prev   previous version (one level of rollback)
//! <cores>/manifest.toml              origin + SHA-256 of each installed core (atomic writes)
//! <cores>/.lock                      inter-process lock (empty, never removed)
//! <cores>/info/<id>_libretro.info    from info.zip
//! <cores>/.cache/index-extended      last index fetched (+ fetch time in manifest.toml)
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    BuildDate, CoreId, CoreInfo, ExtractLimits, FirmwareStatus, KnownFirmware, MAX_INFO_SIZE,
    Platform, RemoteCore, check_firmware, extract_core, parse_index_extended, parse_info,
    read_info_bundle,
};
use crate::AssetError;
use crate::http::{CancelToken, HttpClient, Progress, sync_dir};

/// Where cores live and where firmware is looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDirs {
    /// `<data>/cores/<os>-<arch>/`.
    pub cores: PathBuf,
    /// `<data>/system/` (libretro system directory, BIOS).
    pub system: PathBuf,
}

impl CoreDirs {
    pub fn under(data_root: &Path, platform: Platform) -> Self {
        Self {
            cores: data_root.join("cores").join(platform.dir_name()),
            system: data_root.join("system"),
        }
    }
}

/// Buildbot base URL. Cores are native code: only the official URL unless the user explicitly sets another
/// one in `config.toml` (ADR 0005), in which case the UI shows it permanently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildbotSource {
    base: String,
}

impl BuildbotSource {
    pub const OFFICIAL: &'static str = "https://buildbot.libretro.com";

    pub fn official() -> Self {
        Self {
            base: Self::OFFICIAL.to_owned(),
        }
    }

    /// `https://…`, or `http://127.0.0.1:<port>` (tests; the client still needs `allow_loopback_http`);
    /// trailing `/` removed. Anything else: `AssetError::UrlRefused`.
    pub fn custom(base: &str) -> Result<Self, AssetError> {
        let url = Url::parse(base.trim()).map_err(|_| AssetError::UrlRefused)?;
        let allowed = match url.scheme() {
            "https" => url.host_str().is_some_and(|h| !h.is_empty()),
            "http" => url.host_str() == Some("127.0.0.1"),
            _ => false,
        };
        if !allowed
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AssetError::UrlRefused);
        }
        Ok(Self {
            base: url.as_str().trim_end_matches('/').to_owned(),
        })
    }

    pub fn is_official(&self) -> bool {
        self.base == Self::OFFICIAL
    }

    /// `<base>/nightly/<buildbot_dir>/latest/.index-extended`.
    pub fn index_url(&self, platform: Platform) -> String {
        format!(
            "{}/nightly/{}/latest/.index-extended",
            self.base,
            platform.buildbot_dir()
        )
    }

    /// `<base>/nightly/<buildbot_dir>/latest/<id>_libretro.<ext>.zip`.
    pub fn core_url(&self, platform: Platform, id: &CoreId) -> String {
        format!(
            "{}/nightly/{}/latest/{}.zip",
            self.base,
            platform.buildbot_dir(),
            platform.core_file_name(id)
        )
    }

    /// `<base>/assets/frontend/info.zip`.
    pub fn info_bundle_url(&self) -> String {
        format!("{}/assets/frontend/info.zip", self.base)
    }

    /// The base URL (for display: the UI shows it permanently when not official).
    pub fn base(&self) -> &str {
        &self.base
    }
}

/// Where an installed core came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Installed by the manager; the file must still hash to `sha256`.
    Buildbot {
        url: String,
        date: BuildDate,
        crc32: u32,
        sha256: [u8; 32],
        size: u64,
        installed_unix: u64,
        /// Pinned cores are skipped by "update all".
        pinned: bool,
    },
    /// A `*_libretro.<ext>` file put in `<cores>` by hand: listed and usable, never verified nor updated
    /// (the user can "install" over it to switch to a managed copy).
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledCore {
    pub id: CoreId,
    pub path: PathBuf,
    pub origin: Origin,
    /// `true` when `<file>.prev` exists.
    pub has_previous: bool,
}

/// Remote state, from the last successful [`CoreManager::refresh`] (persisted in `.cache/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCatalog {
    pub fetched_unix: u64,
    pub cores: BTreeMap<CoreId, RemoteCore>,
}

/// An installed buildbot core whose remote CRC differs from the installed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreUpdate {
    pub id: CoreId,
    pub installed: BuildDate,
    pub available: BuildDate,
    pub pinned: bool,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    #[error("plate-forme non prise en charge par le buildbot libretro")]
    UnsupportedPlatform,
    #[error("core `{0}` absent du buildbot pour cette plate-forme")]
    NotAvailable(CoreId),
    #[error("core `{0}` non installé")]
    NotInstalled(CoreId),
    #[error("core `{0}` en cours d'utilisation")]
    InUse(CoreId),
    #[error("core `{0}` : opération déjà en cours")]
    Busy(CoreId),
    #[error("core `{0}` : pas de version précédente")]
    NoPrevious(CoreId),
    /// Installed file no longer matches the SHA-256 recorded at install time: never loaded.
    #[error("core `{id}` modifié depuis son installation ({path}) : réinstaller")]
    Tampered { id: CoreId, path: PathBuf },
    #[error(
        "core `{id}` : le fichier téléchargé (CRC32 {found:08x}) ne correspond pas à l'index du buildbot \
         ({expected:08x}), même après mise à jour du catalogue : réessayez plus tard (build en cours ?) ou \
         vérifiez qu'un proxy/antivirus ne modifie pas les téléchargements"
    )]
    CrcMismatch {
        id: CoreId,
        expected: u32,
        found: u32,
    },
    /// The zip is newer than the index it was checked against: the buildbot is rebuilding it. Nothing installed.
    #[error("core `{0}` : le buildbot est en cours de mise à jour, réessayez plus tard")]
    BuildbotUpdating(CoreId),
    #[error("pas encore de catalogue : vérifier les mises à jour d'abord")]
    NoCatalog,
    /// Network, HTTP status, size limit, cancellation (`AssetError::Cancelled`, added by task A1).
    #[error(transparent)]
    Asset(#[from] AssetError),
    #[error(transparent)]
    Archive(#[from] super::ArchiveError),
    #[error("{path} : {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// `manifest.toml` unreadable: managed cores are then reported as `Manual` (never trusted) and the file
    /// is not rewritten until the user installs or removes a core.
    #[error("manifest des cores invalide : {0}")]
    Manifest(String),
}

/// Size bounds of downloads (bodies are read up to the limit, `Content-Length` is not trusted).
pub const MAX_INDEX_BYTES: u64 = 1024 * 1024;
pub const MAX_INFO_BUNDLE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_CORE_ZIP_BYTES: u64 = 128 * 1024 * 1024;
/// Attempts per download (network errors, timeouts and 5xx only), delays 1 s then 2 s.
pub const DOWNLOAD_ATTEMPTS: u32 = 3;

/// A cached catalog older than this is renewed before an install (offline: kept, the CRC32 check decides).
pub const CATALOG_MAX_AGE: Duration = Duration::from_secs(12 * 3600);

const MANIFEST_FILE: &str = "manifest.toml";
const MANIFEST_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const INFO_DIR: &str = "info";
const CACHE_DIR: &str = ".cache";
const CACHED_INDEX: &str = "index-extended";
const INFO_SUFFIX: &str = "_libretro.info";
const LOCK_FILE: &str = ".lock";
/// Temporary files younger than this may still be written by another process (outside the directory lock) and
/// are left alone by [`CoreManager::open`]. Process ids are not used: pid namespaces differ between containers.
const ORPHAN_AGE: Duration = Duration::from_secs(3600);

#[derive(Debug)]
pub struct CoreManager {
    dirs: CoreDirs,
    platform: Platform,
    source: BuildbotSource,
    http: HttpClient,
    known_firmware: KnownFirmware,
    /// md5 of the free BIOS images of `data/free_bios.toml` (`FirmwareState::Free`).
    free_firmware: Vec<(String, [u8; 16], String)>,
    /// Never held during a download or while hashing a core.
    state: Mutex<State>,
    /// Serializes [`CoreManager::refresh`] (held during its downloads; the UI thread never takes it).
    refresh_lock: Mutex<()>,
}

#[derive(Debug, Default)]
struct State {
    manifest: Manifest,
    /// Last copy read from disk (re-read by every query and under the directory lock before every write).
    /// `false` after a read or parse failure: no entry is trusted (cores are `Manual`) and the file is left as is
    /// until the user installs, removes, rolls back or pins a core.
    manifest_ok: bool,
    catalog: Option<RemoteCatalog>,
    infos: BTreeMap<CoreId, CoreInfo>,
    in_use: Option<CoreId>,
    busy: BTreeSet<CoreId>,
    warned_manual: BTreeSet<CoreId>,
}

impl State {
    fn managed(&self, id: &CoreId) -> Option<&Entry> {
        if self.manifest_ok {
            self.manifest.cores.get(id)
        } else {
            None
        }
    }
}

#[derive(Debug, Default)]
struct Manifest {
    index_fetched_unix: Option<u64>,
    cores: BTreeMap<CoreId, Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    current: Record,
    pinned: bool,
    /// Describes `<file>.prev` when it came from the buildbot.
    previous: Option<Record>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    url: String,
    date: BuildDate,
    crc32: u32,
    sha256: [u8; 32],
    size: u64,
    installed_unix: u64,
}

impl Entry {
    fn origin(&self) -> Origin {
        let r = &self.current;
        Origin::Buildbot {
            url: r.url.clone(),
            date: r.date,
            crc32: r.crc32,
            sha256: r.sha256,
            size: r.size,
            installed_unix: r.installed_unix,
            pinned: self.pinned,
        }
    }
}

impl CoreManager {
    /// `http` should be built with `HttpConfig::timeout` ≥ 10 min (core zips; the default 60 s is for
    /// covers). No network. Creates `<cores>`; under the directory lock (may wait for another process's short
    /// section), removes leftovers of an interrupted run (`.tmp-*` files older than one hour), restores
    /// `<file>` from `<file>.prev` if an update was cut between its two renames; loads manifest, cached
    /// index and `.info` files. An unreadable manifest is logged, not returned: cores are then `Manual`.
    pub fn open(
        dirs: CoreDirs,
        platform: Platform,
        source: BuildbotSource,
        http: HttpClient,
    ) -> Result<Self, CoreError> {
        let info_dir = dirs.cores.join(INFO_DIR);
        let cache_dir = dirs.cores.join(CACHE_DIR);
        fs::create_dir_all(&dirs.cores).map_err(io_err(&dirs.cores))?;
        let dir_lock = DirLock::acquire(&dirs.cores)?;
        for dir in [&dirs.cores, &info_dir, &cache_dir] {
            fs::create_dir_all(dir).map_err(io_err(dir))?;
            remove_leftovers(dir);
        }
        restore_interrupted(&dirs.cores, platform);

        let (manifest, manifest_ok) = match load_manifest(&dirs.cores.join(MANIFEST_FILE), platform)
        {
            Ok(manifest) => (manifest, true),
            Err(e) => {
                tracing::error!("{e}");
                (Manifest::default(), false)
            }
        };
        let catalog = load_catalog(
            &cache_dir.join(CACHED_INDEX),
            platform,
            manifest.index_fetched_unix,
        );
        let infos = load_infos(&info_dir);
        drop(dir_lock);
        Ok(Self {
            dirs,
            platform,
            source,
            http,
            known_firmware: KnownFirmware::builtin(),
            free_firmware: crate::bios::free_firmware_md5(&crate::bios::builtin_free_bios()),
            state: Mutex::new(State {
                manifest,
                manifest_ok,
                catalog,
                infos,
                ..State::default()
            }),
            refresh_lock: Mutex::new(()),
        })
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    pub fn dirs(&self) -> &CoreDirs {
        &self.dirs
    }

    pub fn source(&self) -> &BuildbotSource {
        &self.source
    }

    /// Managed and manual cores, sorted by id. Re-scans the directory and re-reads the manifest (cheap), so
    /// changes made by another process are seen.
    pub fn installed(&self) -> Vec<InstalledCore> {
        let ids = self.scan_installed();
        let mut st = self.lock();
        self.reload_manifest(&mut st);
        ids.iter().map(|id| self.installed_core(&st, id)).collect()
    }

    /// `.info` of a core (installed or not), from the last info bundle.
    pub fn info(&self, id: &CoreId) -> Option<CoreInfo> {
        self.lock().infos.get(id).cloned()
    }

    /// All known `.info`, by id.
    pub fn infos(&self) -> BTreeMap<CoreId, CoreInfo> {
        self.lock().infos.clone()
    }

    pub fn catalog(&self) -> Option<RemoteCatalog> {
        self.lock().catalog.clone()
    }

    /// Network: downloads the index then `info.zip`, replaces `info/` and `.cache/` atomically (per file).
    /// On error the previous catalog and `.info` stay in place. Concurrent calls run one after the other.
    pub fn refresh(
        &self,
        progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<RemoteCatalog, CoreError> {
        let _serial = self
            .refresh_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let index = self.download(
            &self.source.index_url(self.platform),
            MAX_INDEX_BYTES,
            progress,
            cancel,
        )?;
        let index_text = String::from_utf8_lossy(&index).into_owned();
        drop(index);
        let remote = parse_index_extended(&index_text, self.platform);
        if remote.is_empty() {
            return Err(
                AssetError::InvalidResponse("index des cores vide ou illisible".into()).into(),
            );
        }

        let bundle = self.download(
            &self.source.info_bundle_url(),
            MAX_INFO_BUNDLE_BYTES,
            progress,
            cancel,
        )?;
        let entries = read_info_bundle(&bundle, ExtractLimits::INFO_BUNDLE)?;
        drop(bundle);
        let mut infos = BTreeMap::new();
        let mut texts = Vec::with_capacity(entries.len());
        for (id, text) in entries {
            match parse_info(id.clone(), &text) {
                Ok(info) => {
                    infos.insert(id.clone(), info);
                    texts.push((id, text));
                }
                Err(e) => tracing::warn!("info.zip : {id} ignoré ({e})"),
            }
        }
        if infos.is_empty() {
            return Err(AssetError::InvalidResponse("info.zip sans fichier .info".into()).into());
        }
        if cancel.is_cancelled() {
            return Err(AssetError::Cancelled.into());
        }

        let _dir = DirLock::acquire(&self.dirs.cores)?;
        // A re-downloadable cache: renamed atomically but not fsynced one by one (≈ 360 files).
        let info_dir = self.dirs.cores.join(INFO_DIR);
        for (id, text) in &texts {
            write_atomic(
                &info_dir,
                &format!("{id}{INFO_SUFFIX}"),
                text.as_bytes(),
                false,
            )?;
        }
        remove_stale_infos(&info_dir, &infos);
        sync_dir(&info_dir);
        let cache_dir = self.dirs.cores.join(CACHE_DIR);
        write_atomic(&cache_dir, CACHED_INDEX, index_text.as_bytes(), true)?;

        let catalog = RemoteCatalog {
            fetched_unix: now_unix(),
            cores: remote.into_iter().map(|c| (c.id.clone(), c)).collect(),
        };
        let mut st = self.lock();
        st.catalog = Some(catalog.clone());
        st.infos = infos;
        self.reload_manifest(&mut st);
        st.manifest.index_fetched_unix = Some(catalog.fetched_unix);
        // An unreadable manifest is only rewritten by an action on a core; the cache file's mtime stands in.
        if st.manifest_ok
            && let Err(e) = self.write_manifest(&mut st)
        {
            tracing::warn!("date de vérification non enregistrée : {e}");
        }
        Ok(catalog)
    }

    /// From the cached catalog (no network).
    pub fn updates(&self) -> Vec<CoreUpdate> {
        let mut st = self.lock();
        self.reload_manifest(&mut st);
        let st = &*st;
        let Some(catalog) = &st.catalog else {
            return Vec::new();
        };
        if !st.manifest_ok {
            return Vec::new();
        }
        st.manifest
            .cores
            .iter()
            .filter(|(id, _)| self.core_path(id).exists())
            .filter_map(|(id, entry)| {
                let remote = catalog.cores.get(id)?;
                (remote.crc32 != entry.current.crc32).then(|| CoreUpdate {
                    id: id.clone(),
                    installed: entry.current.date,
                    available: remote.date,
                    pinned: entry.pinned,
                })
            })
            .collect()
    }

    /// Installs or updates `id` from the cached catalog (`NoCatalog` if none):
    /// download (`MAX_CORE_ZIP_BYTES`, retries) → [`super::extract_core`] → CRC32 against the index → SHA-256 →
    /// temporary file in `<cores>` → `sync_all` → current file renamed to `.prev` (replacing the old `.prev`)
    /// → temporary renamed to the final name → directory fsync (Unix) → manifest written atomically (renames and
    /// manifest under the directory lock).
    /// Refused with `InUse` for the core set by [`Self::set_in_use`], `Busy` if already running for `id`.
    /// Any failure or cancellation leaves the previous installation untouched and no temporary file.
    pub fn install(
        &self,
        id: &CoreId,
        progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<InstalledCore, CoreError> {
        {
            let mut st = self.lock();
            check_free(&st, id)?;
            if st.catalog.is_none() {
                return Err(CoreError::NoCatalog);
            }
            st.busy.insert(id.clone());
        }
        let _busy = BusyGuard { manager: self, id };

        // Nightlies are rebuilt daily: an old cached index would announce a CRC32 that no longer matches.
        let mut refreshed = false;
        let stale = self
            .lock()
            .catalog
            .as_ref()
            .is_some_and(|c| catalog_is_stale(c.fetched_unix, now_unix()));
        if stale {
            refreshed = self.refresh_for_install(progress, cancel)?;
        }
        let url = self.source.core_url(self.platform, id);
        let file_name = self.platform.core_file_name(id);
        let mut remote = self.remote_core(id)?;
        let mut index_fetched = self.catalog_fetched_unix();
        let (data, crc32) = loop {
            let (zip, last_modified) =
                self.download_meta(&url, MAX_CORE_ZIP_BYTES, progress, cancel)?;
            let data = extract_core(&zip, &file_name, ExtractLimits::CORE)?;
            drop(zip);
            let crc32 = crc32fast::hash(&data);
            if crc32 == remote.crc32 {
                break (data, crc32);
            }
            // The index may predate the zip: one refresh, then the download is verified against the fresh index.
            // Never accept a file that disagrees with an index fetched after the download.
            let previous = remote.clone();
            if refreshed || !self.refresh_for_install(progress, cancel)? {
                return Err(crc_failure(
                    id,
                    &remote,
                    crc32,
                    last_modified,
                    index_fetched,
                ));
            }
            refreshed = true;
            remote = self.remote_core(id)?;
            if crc32 == remote.crc32 {
                // Stale index, recent zip: the download already matches the fresh index.
                break (data, crc32);
            }
            drop(data);
            if remote.crc32 == previous.crc32 {
                return Err(crc_failure(
                    id,
                    &remote,
                    crc32,
                    last_modified,
                    index_fetched,
                ));
            }
            index_fetched = self.catalog_fetched_unix();
        };
        if cancel.is_cancelled() {
            return Err(AssetError::Cancelled.into());
        }
        let record = Record {
            url,
            date: remote.date,
            crc32,
            sha256: Sha256::digest(&data).into(),
            size: data.len() as u64,
            installed_unix: now_unix(),
        };
        let temp = write_temp(&self.dirs.cores, &file_name, &data, true)?;
        drop(data);
        self.commit_install(id, temp, record, cancel)
    }

    fn catalog_fetched_unix(&self) -> u64 {
        self.lock().catalog.as_ref().map_or(0, |c| c.fetched_unix)
    }

    fn remote_core(&self, id: &CoreId) -> Result<RemoteCore, CoreError> {
        let st = self.lock();
        let catalog = st.catalog.as_ref().ok_or(CoreError::NoCatalog)?;
        catalog
            .cores
            .get(id)
            .cloned()
            .ok_or_else(|| CoreError::NotAvailable(id.clone()))
    }

    /// `Ok(true)` when the catalog was renewed. Offline (any error but cancellation) the cached catalog is kept
    /// and `Ok(false)` is returned: the CRC32 check then decides.
    fn refresh_for_install(
        &self,
        progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<bool, CoreError> {
        match self.refresh(progress, cancel) {
            Ok(_) => Ok(true),
            Err(CoreError::Asset(AssetError::Cancelled)) => Err(AssetError::Cancelled.into()),
            Err(e) => {
                tracing::warn!("catalogue non rafraîchi avant l'installation : {e}");
                Ok(false)
            }
        }
    }

    /// Renames and manifest, under the directory lock and the state lock (no network, a few file system calls).
    fn commit_install(
        &self,
        id: &CoreId,
        mut temp: TempFile,
        record: Record,
        cancel: &CancelToken,
    ) -> Result<InstalledCore, CoreError> {
        let _dir = DirLock::acquire(&self.dirs.cores)?;
        let mut st = self.lock();
        // The UI may have started this core while it was downloading.
        if st.in_use.as_ref() == Some(id) {
            return Err(CoreError::InUse(id.clone()));
        }
        if cancel.is_cancelled() {
            return Err(AssetError::Cancelled.into());
        }
        self.reload_manifest(&mut st);
        let final_path = self.core_path(id);
        let prev_path = self.prev_path(id);
        let had_current = fs::symlink_metadata(&final_path).is_ok();
        if had_current {
            fs::rename(&final_path, &prev_path).map_err(io_err(&final_path))?;
        }
        if let Err(e) = fs::rename(&temp.path, &final_path) {
            if had_current {
                let _ = fs::rename(&prev_path, &final_path);
            }
            return Err(io_err(&final_path)(e));
        }
        temp.armed = false;
        sync_dir(&self.dirs.cores);

        let old = st.managed(id).cloned();
        let entry = Entry {
            current: record,
            pinned: old.as_ref().is_some_and(|e| e.pinned),
            // A replaced manual file has no record: its `.prev` will be manual again after a rollback.
            previous: old.filter(|_| had_current).map(|e| e.current),
        };
        if !st.manifest_ok {
            // User action on an unreadable manifest: start a new one (ADR 0013).
            st.manifest.cores.clear();
        }
        let replaced = st.manifest.cores.insert(id.clone(), entry);
        if let Err(e) = self.write_manifest(&mut st) {
            // File and manifest must agree: put the previous file back (its own `.prev` is lost).
            match replaced {
                Some(entry) => st.manifest.cores.insert(id.clone(), entry),
                None => st.manifest.cores.remove(id),
            };
            let _ = fs::remove_file(&final_path);
            if had_current {
                let _ = fs::rename(&prev_path, &final_path);
            }
            sync_dir(&self.dirs.cores);
            return Err(e);
        }
        tracing::info!("core {id} installé");
        Ok(self.installed_core(&st, id))
    }

    /// Deletes the core, its `.prev` and its manifest entry (`.info`, saves, states and options are kept).
    pub fn remove(&self, id: &CoreId) -> Result<(), CoreError> {
        let _dir = DirLock::acquire(&self.dirs.cores)?;
        let mut st = self.lock();
        check_free(&st, id)?;
        self.reload_manifest(&mut st);
        let current = remove_if_exists(&self.core_path(id))?;
        let previous = remove_if_exists(&self.prev_path(id))?;
        sync_dir(&self.dirs.cores);
        let had_entry = st.manifest_ok && st.manifest.cores.remove(id).is_some();
        if had_entry {
            self.write_manifest(&mut st)?;
        }
        if !(current || previous || had_entry) {
            return Err(CoreError::NotInstalled(id.clone()));
        }
        tracing::info!("core {id} supprimé");
        Ok(())
    }

    /// Swaps the current file and `.prev` (manifest entries swapped too).
    ///
    /// Refused with [`CoreError::Tampered`] when the current file of a managed core no longer matches the
    /// manifest (modified by hand, or an install cut between its renames and its manifest write): the records
    /// of both files are then unreliable and a rollback would not repair anything. The UI should offer to
    /// reinstall (or remove) the core instead. Hashes the current file: call it from a job or accept a short wait.
    pub fn rollback(&self, id: &CoreId) -> Result<InstalledCore, CoreError> {
        let _dir = DirLock::acquire(&self.dirs.cores)?;
        let record = {
            let mut st = self.lock();
            check_free(&st, id)?;
            self.reload_manifest(&mut st);
            st.managed(id).map(|e| e.current.clone())
        };
        let final_path = self.core_path(id);
        let prev_path = self.prev_path(id);
        let had_current = fs::symlink_metadata(&final_path).is_ok();
        if fs::symlink_metadata(&prev_path).is_err() {
            return Err(if had_current {
                CoreError::NoPrevious(id.clone())
            } else {
                CoreError::NotInstalled(id.clone())
            });
        }
        // Hashed without the state mutex; the directory lock keeps files and manifest still meanwhile.
        if had_current && let Some(record) = &record {
            check_intact(id, &final_path, record)?;
        }
        let mut st = self.lock();
        check_free(&st, id)?;
        self.swap_with_prev(id, had_current)?;

        let old = if st.manifest_ok {
            st.manifest.cores.remove(id)
        } else {
            None
        };
        let Some(old) = old else {
            // Nothing recorded (manual, or unreadable manifest): both files stay unverified.
            return Ok(self.installed_core(&st, id));
        };
        if let Some(previous) = old.previous.clone() {
            st.manifest.cores.insert(
                id.clone(),
                Entry {
                    current: previous,
                    pinned: old.pinned,
                    previous: had_current.then(|| old.current.clone()),
                },
            );
        }
        if let Err(e) = self.write_manifest(&mut st) {
            st.manifest.cores.insert(id.clone(), old);
            let undo = if had_current {
                self.swap_with_prev(id, true)
            } else {
                // `.prev` was renamed to the current file: rename it back.
                fs::rename(&final_path, &prev_path)
                    .map_err(io_err(&final_path))
                    .map(|()| sync_dir(&self.dirs.cores))
            };
            if let Err(undo) = undo {
                tracing::error!("core {id} : retour arrière non annulé : {undo}");
            }
            return Err(e);
        }
        tracing::info!("core {id} : version précédente restaurée");
        Ok(self.installed_core(&st, id))
    }

    /// `<file>` ⇄ `<file>.prev` through a temporary name. A crash in between leaves `<file>` absent and is
    /// repaired by [`Self::open`] (`.prev` restored, temporary removed).
    ///
    /// Known limit: after a crash between the first and the last rename, the former `.prev` only survives as the
    /// temporary `.<file>.tmp-*`, which [`Self::open`] deletes once older than [`ORPHAN_AGE`] (1 h): that previous
    /// version is then lost (the current one is intact). Accepted (docs/questions-ouvertes.md).
    fn swap_with_prev(&self, id: &CoreId, had_current: bool) -> Result<(), CoreError> {
        let final_path = self.core_path(id);
        let prev_path = self.prev_path(id);
        if !had_current {
            fs::rename(&prev_path, &final_path).map_err(io_err(&prev_path))?;
            sync_dir(&self.dirs.cores);
            return Ok(());
        }
        let temp = temp_path(&self.dirs.cores, &self.platform.core_file_name(id));
        fs::rename(&prev_path, &temp).map_err(io_err(&prev_path))?;
        if let Err(e) = fs::rename(&final_path, &prev_path) {
            let _ = fs::rename(&temp, &prev_path);
            return Err(io_err(&final_path)(e));
        }
        if let Err(e) = fs::rename(&temp, &final_path) {
            let _ = fs::rename(&prev_path, &final_path);
            let _ = fs::rename(&temp, &prev_path);
            return Err(io_err(&final_path)(e));
        }
        sync_dir(&self.dirs.cores);
        Ok(())
    }

    pub fn set_pinned(&self, id: &CoreId, pinned: bool) -> Result<(), CoreError> {
        let _dir = DirLock::acquire(&self.dirs.cores)?;
        let mut st = self.lock();
        self.reload_manifest(&mut st);
        if !st.manifest_ok || !self.core_path(id).exists() {
            return Err(CoreError::NotInstalled(id.clone()));
        }
        let Some(entry) = st.manifest.cores.get_mut(id) else {
            return Err(CoreError::NotInstalled(id.clone()));
        };
        let before = std::mem::replace(&mut entry.pinned, pinned);
        if let Err(e) = self.write_manifest(&mut st) {
            if let Some(entry) = st.manifest.cores.get_mut(id) {
                entry.pinned = before;
            }
            return Err(e);
        }
        Ok(())
    }

    /// The core the UI is about to load or has loaded (`None` once unloaded): `install`, `remove` and
    /// `rollback` refuse it. Required on Windows, where a loaded DLL cannot be replaced.
    pub fn set_in_use(&self, id: Option<CoreId>) {
        self.lock().in_use = id;
    }

    /// Path to give to the runtime. Managed core: SHA-256 recomputed and compared to the manifest re-read from
    /// disk (`Tampered` on mismatch). Manual core: returned as is (`warn!` once).
    /// Call [`Self::set_in_use`] first: the file can then no longer be replaced by this manager (another process
    /// sharing the directory still can: nothing prevents it between this check and the load).
    pub fn verified_path(&self, id: &CoreId) -> Result<PathBuf, CoreError> {
        match self.verify(id) {
            // Another process may be between its renames and its manifest write: check again once it is done.
            Err(CoreError::Tampered { .. }) => {
                let _dir = DirLock::acquire(&self.dirs.cores)?;
                self.verify(id).inspect_err(|e| {
                    if matches!(e, CoreError::Tampered { .. }) {
                        tracing::error!(
                            "core {id} : SHA-256 différent du manifeste, chargement refusé"
                        );
                    }
                })
            }
            other => other,
        }
    }

    fn verify(&self, id: &CoreId) -> Result<PathBuf, CoreError> {
        let path = self.core_path(id);
        if !fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            return Err(CoreError::NotInstalled(id.clone()));
        }
        let record = {
            let mut st = self.lock();
            self.reload_manifest(&mut st);
            let record = st.managed(id).map(|e| e.current.clone());
            if record.is_none() && st.warned_manual.insert(id.clone()) {
                tracing::warn!("core {id} manuel : chargé sans vérification d'intégrité");
            }
            record
        };
        if let Some(record) = record {
            check_intact(id, &path, &record)?;
        }
        Ok(path)
    }

    /// Firmware listed by the core's `.info`, checked in `dirs.system` (no network, nothing downloaded).
    /// Empty when the `.info` is unknown (refresh first). Any image of `data/free_bios.toml` under its
    /// `install_as` name is `FirmwareState::Free`, whether emu-rust installed it or the user copied it.
    pub fn firmware_status(&self, id: &CoreId) -> Vec<FirmwareStatus> {
        let Some(info) = self.info(id) else {
            return Vec::new();
        };
        check_firmware(
            &self.dirs.system,
            &info,
            &self.known_firmware,
            &self.free_firmware,
        )
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn core_path(&self, id: &CoreId) -> PathBuf {
        self.dirs.cores.join(self.platform.core_file_name(id))
    }

    fn prev_path(&self, id: &CoreId) -> PathBuf {
        self.dirs
            .cores
            .join(format!("{}.prev", self.platform.core_file_name(id)))
    }

    fn installed_core(&self, st: &State, id: &CoreId) -> InstalledCore {
        InstalledCore {
            id: id.clone(),
            path: self.core_path(id),
            origin: st.managed(id).map_or(Origin::Manual, Entry::origin),
            has_previous: fs::symlink_metadata(self.prev_path(id)).is_ok(),
        }
    }

    /// `*_libretro.<ext>` regular files (symlinks followed), sorted.
    fn scan_installed(&self) -> Vec<CoreId> {
        let ext = self.platform.library_extension();
        let Ok(entries) = fs::read_dir(&self.dirs.cores) else {
            return Vec::new();
        };
        let mut ids: Vec<CoreId> = entries
            .filter_map(Result::ok)
            .filter(|e| fs::metadata(e.path()).is_ok_and(|m| m.is_file()))
            .filter_map(|e| CoreId::from_library_file(e.file_name().to_str()?, ext))
            .collect();
        ids.sort();
        ids
    }

    /// Replaces the in-memory manifest with the file's content (written by this or another process).
    fn reload_manifest(&self, st: &mut State) {
        match load_manifest(&self.dirs.cores.join(MANIFEST_FILE), self.platform) {
            Ok(manifest) => {
                st.manifest = manifest;
                st.manifest_ok = true;
            }
            Err(e) => {
                if st.manifest_ok {
                    tracing::error!("{e}");
                }
                st.manifest = Manifest::default();
                st.manifest_ok = false;
            }
        }
    }

    /// Caller holds the directory lock and has called [`Self::reload_manifest`] in the same section.
    fn write_manifest(&self, st: &mut State) -> Result<(), CoreError> {
        let file = ManifestFile::from_manifest(&st.manifest, self.platform);
        let text = toml::to_string(&file).map_err(|e| CoreError::Manifest(e.to_string()))?;
        write_atomic(&self.dirs.cores, MANIFEST_FILE, text.as_bytes(), true)?;
        st.manifest_ok = true;
        Ok(())
    }

    /// GET with retries on transient errors (network, timeout, 5xx), 1 s then 2 s apart.
    fn download(
        &self,
        url: &str,
        max_bytes: u64,
        progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<Vec<u8>, AssetError> {
        self.download_meta(url, max_bytes, progress, cancel)
            .map(|(body, _)| body)
    }

    /// [`Self::download`] also returning the response's `Last-Modified` (Unix seconds).
    fn download_meta(
        &self,
        url: &str,
        max_bytes: u64,
        progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<(Vec<u8>, Option<u64>), AssetError> {
        let mut attempt = 1;
        loop {
            match self
                .http
                .get_with_last_modified(url, max_bytes, progress, cancel)
            {
                Err(e) if e.is_transient() && attempt < DOWNLOAD_ATTEMPTS => {
                    tracing::warn!(
                        "téléchargement : {e} (tentative {attempt}/{DOWNLOAD_ATTEMPTS})"
                    );
                    wait(Duration::from_secs(u64::from(attempt)), cancel)?;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }
}

/// `BuildbotUpdating` when the zip was modified after the index was fetched: the buildbot rebuilt it since. Otherwise a plain `CrcMismatch`. The file is never installed.
fn crc_failure(
    id: &CoreId,
    remote: &RemoteCore,
    found: u32,
    last_modified: Option<u64>,
    index_fetched: u64,
) -> CoreError {
    if last_modified.is_some_and(|lm| lm > index_fetched) {
        CoreError::BuildbotUpdating(id.clone())
    } else {
        CoreError::CrcMismatch {
            id: id.clone(),
            expected: remote.crc32,
            found,
        }
    }
}

fn catalog_is_stale(fetched_unix: u64, now: u64) -> bool {
    now.saturating_sub(fetched_unix) > CATALOG_MAX_AGE.as_secs()
}

fn check_free(st: &State, id: &CoreId) -> Result<(), CoreError> {
    if st.in_use.as_ref() == Some(id) {
        return Err(CoreError::InUse(id.clone()));
    }
    if st.busy.contains(id) {
        return Err(CoreError::Busy(id.clone()));
    }
    Ok(())
}

/// `Tampered` unless `path` has the size and SHA-256 of `record`.
fn check_intact(id: &CoreId, path: &Path, record: &Record) -> Result<(), CoreError> {
    match sha256_file(path, record.size) {
        Ok(Some(sha)) if sha == record.sha256 => Ok(()),
        Ok(_) => Err(CoreError::Tampered {
            id: id.clone(),
            path: path.to_path_buf(),
        }),
        Err(e) => Err(io_err(path)(e)),
    }
}

/// Exclusive lock on `<cores>/.lock`, released on drop. Each acquisition opens its own handle, so it also
/// serializes threads and managers of one process; never acquire it twice in the same call chain.
struct DirLock {
    _file: Option<File>,
}

impl DirLock {
    fn acquire(cores: &Path) -> Result<Self, CoreError> {
        let path = cores.join(LOCK_FILE);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io_err(&path))?;
        match file.lock() {
            Ok(()) => Ok(Self { _file: Some(file) }),
            // A file system without locks (some network mounts): work as before rather than not at all.
            Err(e) if lock_unsupported(&e) => {
                static WARNED: std::sync::Once = std::sync::Once::new();
                WARNED.call_once(|| {
                    tracing::warn!(
                        "{} : verrou non pris en charge, accès concurrent non protégé",
                        path.display()
                    );
                });
                Ok(Self { _file: None })
            }
            Err(e) => Err(io_err(&path)(e)),
        }
    }
}

/// `Unsupported`, or `ENOLCK` (NFS without a lock daemon, some FUSE mounts), which std reports as `Other`.
fn lock_unsupported(error: &io::Error) -> bool {
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::ENOLCK) {
        return true;
    }
    error.kind() == io::ErrorKind::Unsupported
}

struct BusyGuard<'a> {
    manager: &'a CoreManager,
    id: &'a CoreId,
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.manager.lock().busy.remove(self.id);
    }
}

fn wait(duration: Duration, cancel: &CancelToken) -> Result<(), AssetError> {
    let end = std::time::Instant::now() + duration;
    loop {
        if cancel.is_cancelled() {
            return Err(AssetError::Cancelled);
        }
        let left = end.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        std::thread::sleep(left.min(Duration::from_millis(50)));
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> CoreError + '_ {
    move |source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Files

/// A temporary file removed on drop unless `armed` is cleared (after its rename).
#[derive(Debug)]
struct TempFile {
    path: PathBuf,
    armed: bool,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed
            && let Err(e) = fs::remove_file(&self.path)
            && e.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!("{} : suppression impossible : {e}", self.path.display());
        }
    }
}

/// `.<name>.tmp-<pid>-<n>`: matched by [`remove_leftovers`], unique within the process.
fn temp_path(dir: &Path, name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{name}.tmp-{}-{n}", std::process::id()))
}

fn write_temp(dir: &Path, name: &str, data: &[u8], durable: bool) -> Result<TempFile, CoreError> {
    let path = temp_path(dir, name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(io_err(&path))?;
    let temp = TempFile { path, armed: true };
    file.write_all(data).map_err(io_err(&temp.path))?;
    if durable {
        file.sync_all().map_err(io_err(&temp.path))?;
    }
    Ok(temp)
}

fn write_atomic(dir: &Path, name: &str, data: &[u8], durable: bool) -> Result<(), CoreError> {
    let mut temp = write_temp(dir, name, data, durable)?;
    let dest = dir.join(name);
    fs::rename(&temp.path, &dest).map_err(io_err(&dest))?;
    temp.armed = false;
    if durable {
        sync_dir(dir);
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<bool, CoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_err(path)(e)),
    }
}

/// Reads at most `max` bytes; larger files are an error (never allocated).
fn read_bounded(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    File::open(path)?.take(max + 1).read_to_end(&mut data)?;
    if data.len() as u64 > max {
        return Err(io::Error::other("fichier trop volumineux"));
    }
    Ok(data)
}

/// `None` when the file is not exactly `expected_size` bytes long.
fn sha256_file(path: &Path, expected_size: u64) -> io::Result<Option<[u8; 32]>> {
    let mut hasher = Sha256::new();
    let read = io::copy(&mut File::open(path)?.take(expected_size + 1), &mut hasher)?;
    Ok((read == expected_size).then(|| hasher.finalize().into()))
}

/// Temporary files of an interrupted run (`.*.tmp-*`, regular files only, older than [`ORPHAN_AGE`]: younger
/// ones may belong to another process). Called under the directory lock: no rename of this manager is in flight.
fn remove_leftovers(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.')
            && name.contains(".tmp-")
            && entry.file_type().is_ok_and(|t| t.is_file())
            && entry.metadata().is_ok_and(|m| is_orphan(&m))
        {
            match fs::remove_file(entry.path()) {
                Ok(()) => tracing::info!("résidu supprimé : {}", entry.path().display()),
                Err(e) => tracing::warn!("{} : {e}", entry.path().display()),
            }
        }
    }
}

fn is_orphan(meta: &fs::Metadata) -> bool {
    // An mtime in the future (clock change) or unavailable: kept.
    meta.modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= ORPHAN_AGE)
}

/// An update cut between its two renames leaves `<file>.prev` without `<file>`: put it back.
fn restore_interrupted(cores: &Path, platform: Platform) {
    let Ok(entries) = fs::read_dir(cores) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(base) = name.to_str().and_then(|n| n.strip_suffix(".prev")) else {
            continue;
        };
        if CoreId::from_library_file(base, platform.library_extension()).is_none() {
            continue;
        }
        let target = cores.join(base);
        if fs::symlink_metadata(&target).is_err() {
            match fs::rename(entry.path(), &target) {
                Ok(()) => tracing::warn!("installation interrompue : {base} restauré"),
                Err(e) => tracing::error!("{} : {e}", target.display()),
            }
        }
    }
    sync_dir(cores);
}

fn load_infos(dir: &Path) -> BTreeMap<CoreId, CoreInfo> {
    let mut infos = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return infos;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|n| n.strip_suffix(INFO_SUFFIX))
            .and_then(CoreId::new)
        else {
            continue;
        };
        let parsed = read_bounded(&entry.path(), MAX_INFO_SIZE as u64)
            .map_err(|e| e.to_string())
            .and_then(|data| {
                parse_info(id.clone(), &String::from_utf8_lossy(&data)).map_err(|e| e.to_string())
            });
        match parsed {
            Ok(info) => {
                infos.insert(id, info);
            }
            Err(e) => tracing::warn!("{} : {e}", entry.path().display()),
        }
    }
    infos
}

fn remove_stale_infos(dir: &Path, keep: &BTreeMap<CoreId, CoreInfo>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let stale = name
            .to_str()
            .and_then(|n| n.strip_suffix(INFO_SUFFIX))
            .and_then(CoreId::new)
            .is_some_and(|id| !keep.contains_key(&id));
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn load_catalog(
    path: &Path,
    platform: Platform,
    fetched_unix: Option<u64>,
) -> Option<RemoteCatalog> {
    let data = match read_bounded(path, MAX_INDEX_BYTES) {
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!("{} : {e}", path.display());
            return None;
        }
    };
    let cores = parse_index_extended(&String::from_utf8_lossy(&data), platform);
    if cores.is_empty() {
        return None;
    }
    let fetched_unix = fetched_unix
        .or_else(|| {
            let modified = fs::metadata(path).and_then(|m| m.modified()).ok()?;
            modified
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
        })
        .unwrap_or(0);
    Some(RemoteCatalog {
        fetched_unix,
        cores: cores.into_iter().map(|c| (c.id.clone(), c)).collect(),
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Manifest (docs/design/v1.0-cores.md §2.2)

#[derive(Serialize, Deserialize)]
struct ManifestFile {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index_fetched_unix: Option<u64>,
    #[serde(default)]
    cores: BTreeMap<String, EntryFile>,
}

#[derive(Serialize, Deserialize)]
struct EntryFile {
    #[serde(flatten)]
    current: RecordFile,
    #[serde(default)]
    pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<RecordFile>,
}

#[derive(Serialize, Deserialize)]
struct RecordFile {
    file: String,
    url: String,
    date: String,
    crc32: String,
    sha256: String,
    size: u64,
    installed_unix: u64,
}

impl ManifestFile {
    fn from_manifest(manifest: &Manifest, platform: Platform) -> Self {
        let record = |id: &CoreId, r: &Record| RecordFile {
            file: platform.core_file_name(id),
            url: r.url.clone(),
            date: r.date.to_string(),
            crc32: format!("{:08x}", r.crc32),
            sha256: hex(&r.sha256),
            size: r.size,
            installed_unix: r.installed_unix,
        };
        Self {
            version: MANIFEST_VERSION,
            index_fetched_unix: manifest.index_fetched_unix,
            cores: manifest
                .cores
                .iter()
                .map(|(id, e)| {
                    (
                        id.to_string(),
                        EntryFile {
                            current: record(id, &e.current),
                            pinned: e.pinned,
                            previous: e.previous.as_ref().map(|p| record(id, p)),
                        },
                    )
                })
                .collect(),
        }
    }
}

/// Invalid entries are dropped (their cores become `Manual`); a wrong version or bad syntax fails the whole file.
fn load_manifest(path: &Path, platform: Platform) -> Result<Manifest, CoreError> {
    let data = match read_bounded(path, MAX_MANIFEST_BYTES) {
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Manifest::default()),
        Err(e) => return Err(CoreError::Manifest(e.to_string())),
    };
    let text = std::str::from_utf8(&data).map_err(|e| CoreError::Manifest(e.to_string()))?;
    let file: ManifestFile =
        toml::from_str(text).map_err(|e| CoreError::Manifest(e.message().to_owned()))?;
    if file.version != MANIFEST_VERSION {
        return Err(CoreError::Manifest(format!(
            "version {} inconnue",
            file.version
        )));
    }
    let mut cores = BTreeMap::new();
    for (key, entry) in file.cores {
        let Some(id) = CoreId::new(&key) else {
            tracing::warn!("manifest : identifiant invalide ignoré");
            continue;
        };
        let expected = platform.core_file_name(&id);
        let Some(current) = record_from_file(&entry.current, &expected) else {
            tracing::warn!("manifest : entrée {id} invalide, core considéré comme manuel");
            continue;
        };
        let previous = entry
            .previous
            .as_ref()
            .and_then(|p| record_from_file(p, &expected));
        cores.insert(
            id,
            Entry {
                current,
                pinned: entry.pinned,
                previous,
            },
        );
    }
    Ok(Manifest {
        index_fetched_unix: file.index_fetched_unix,
        cores,
    })
}

fn record_from_file(r: &RecordFile, expected_file: &str) -> Option<Record> {
    if r.file != expected_file || r.crc32.len() != 8 {
        return None;
    }
    Some(Record {
        url: r.url.clone(),
        date: BuildDate::parse(&r.date)?,
        crc32: u32::from_str_radix(&r.crc32, 16).ok()?,
        sha256: parse_sha256(&r.sha256)?,
        size: r.size,
        installed_unix: r.installed_unix,
    })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn parse_sha256(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests;
