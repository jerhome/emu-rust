//! Core manager end to end against a simulated buildbot (wiremock): `.index-extended`, `info.zip` and zipped
//! cores. The cores are real shared libraries built from `tests/mock-core` (two variants, so that "old" and
//! "new" differ). Unix only: the mock core needs a C compiler.
#![cfg(unix)]

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use emu_assets::AssetError;
use emu_assets::cores::{
    ArchiveError, BuildbotSource, CoreDirs, CoreError, CoreId, CoreManager, Origin, Platform,
};
use emu_assets::http::{CancelToken, HttpClient, HttpConfig};
use emu_assets::jobs::{Job, JobEvent};
use mock_core::Variant;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::write::SimpleFileOptions;

const P: Platform = Platform::LinuxX86_64;
const INDEX: &str = "/nightly/linux/x86_64/latest/.index-extended";
const INFO_ZIP: &str = "/assets/frontend/info.zip";
const GAMBATTE_ZIP: &str = "/nightly/linux/x86_64/latest/gambatte_libretro.so.zip";
const INFO: &str = "display_name = \"Nintendo - Game Boy / Color (Gambatte)\"\n\
    supported_extensions = \"gb|gbc\"\nlicense = \"GPLv2\"\n\
    database = \"Nintendo - Game Boy|Nintendo - Game Boy Color\"\n";

struct Buildbot {
    server: MockServer,
    rt: tokio::runtime::Runtime,
}

impl Buildbot {
    fn start() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let server = rt.block_on(MockServer::start());
        let bot = Self { server, rt };
        bot.serve_info();
        bot
    }

    fn mount(&self, mock: Mock) {
        self.rt.block_on(mock.mount(&self.server));
    }

    fn serve_info(&self) {
        self.mount(
            Mock::given(method("GET")).and(path(INFO_ZIP)).respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(zip_of(&[("gambatte_libretro.info", INFO.as_bytes())])),
            ),
        );
    }

    /// Replaces the buildbot's content: the index lists `gambatte` with `index_crc`; its zip holds `entry`.
    fn publish(&self, date: &str, index_crc: u32, entry: &str, core: &[u8]) {
        self.publish_raw(date, index_crc, zip_of(&[(entry, core)]));
    }

    fn publish_raw(&self, date: &str, index_crc: u32, zip: Vec<u8>) {
        self.publish_zip(
            date,
            index_crc,
            ResponseTemplate::new(200).set_body_bytes(zip),
        );
    }

    fn publish_zip(&self, date: &str, index_crc: u32, zip_response: ResponseTemplate) {
        self.rt.block_on(self.server.reset());
        self.serve_info();
        self.mount(
            Mock::given(path(INDEX)).respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(format!("{date} {index_crc:08x} gambatte_libretro.so.zip\n")),
            ),
        );
        self.mount(Mock::given(path(GAMBATTE_ZIP)).respond_with(zip_response));
    }

    /// Like `publish`, the core zip being served with a `Last-Modified` header.
    fn publish_with_last_modified(&self, index_crc: u32, core: &[u8], modified: &str) {
        self.publish_zip(
            "2026-11-20",
            index_crc,
            ResponseTemplate::new(200)
                .insert_header("Last-Modified", modified)
                .set_body_bytes(zip_of(&[("gambatte_libretro.so", core)])),
        );
    }

    fn publish_good(&self, date: &str, core: &[u8]) {
        self.publish(date, crc32fast::hash(core), "gambatte_libretro.so", core);
    }

    fn uri(&self) -> String {
        self.server.uri()
    }
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn id(s: &str) -> CoreId {
    CoreId::new(s).unwrap()
}

fn open(bot: &Buildbot, data: &Path) -> CoreManager {
    let http = HttpClient::new(HttpConfig {
        allow_loopback_http: true,
        timeout: Duration::from_secs(20),
        ..HttpConfig::default()
    })
    .unwrap();
    let source = BuildbotSource::custom(&bot.uri()).unwrap();
    CoreManager::open(CoreDirs::under(data, P), P, source, http).unwrap()
}

fn refresh(m: &CoreManager) -> Result<emu_assets::cores::RemoteCatalog, CoreError> {
    m.refresh(&mut |_| {}, &CancelToken::new())
}

fn install(m: &CoreManager, core: &str) -> Result<emu_assets::cores::InstalledCore, CoreError> {
    m.install(&id(core), &mut |_| {}, &CancelToken::new())
}

fn cores_dir(data: &Path) -> PathBuf {
    CoreDirs::under(data, P).cores
}

fn mock_core_bytes(variant: Variant) -> Vec<u8> {
    fs::read(mock_core::build(variant)).unwrap()
}

fn tmp_leftovers(data: &Path) -> Vec<String> {
    let root = cores_dir(data);
    let mut found = Vec::new();
    for dir in [root.clone(), root.join("info"), root.join(".cache")] {
        for e in fs::read_dir(dir).unwrap() {
            let name = e.unwrap().file_name().into_string().unwrap();
            if name.contains(".tmp-") {
                found.push(name);
            }
        }
    }
    found
}

#[test]
fn full_lifecycle_with_real_mock_cores() {
    let old = mock_core_bytes(Variant::Default);
    let new = mock_core_bytes(Variant::Rgb565);
    assert_ne!(old, new);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    let gambatte = id("gambatte");
    let core_file = cores_dir(data.path()).join("gambatte_libretro.so");

    // Nothing without refresh.
    assert!(m.installed().is_empty());
    assert!(matches!(install(&m, "gambatte"), Err(CoreError::NoCatalog)));

    // refresh
    bot.publish_good("2026-11-10", &old);
    let catalog = refresh(&m).unwrap();
    assert_eq!(catalog.cores.len(), 1);
    assert_eq!(catalog.cores[&gambatte].crc32, crc32fast::hash(&old));
    assert!(m.info(&gambatte).is_some());
    assert!(matches!(
        install(&m, "mesen"),
        Err(CoreError::NotAvailable(_))
    ));

    // install: file identical to the zipped core, manifest recorded, verified path usable
    let installed = install(&m, "gambatte").unwrap();
    assert_eq!(installed.path, core_file);
    assert!(!installed.has_previous);
    assert!(matches!(installed.origin, Origin::Buildbot { .. }));
    assert_eq!(fs::read(&core_file).unwrap(), old);
    assert_eq!(m.verified_path(&gambatte).unwrap(), core_file);
    assert!(m.updates().is_empty());
    assert!(tmp_leftovers(data.path()).is_empty());

    // Persistence: a new manager sees the install and the cached catalog without network.
    drop(m);
    let m = open(&bot, data.path());
    assert!(matches!(m.installed()[0].origin, Origin::Buildbot { .. }));
    assert!(m.catalog().is_some());
    assert!(m.info(&gambatte).is_some());
    m.verified_path(&gambatte).unwrap();

    // set_in_use protects the loaded core
    m.set_in_use(Some(gambatte.clone()));
    assert!(matches!(install(&m, "gambatte"), Err(CoreError::InUse(_))));
    assert!(matches!(m.remove(&gambatte), Err(CoreError::InUse(_))));
    assert!(matches!(m.rollback(&gambatte), Err(CoreError::InUse(_))));
    assert_eq!(fs::read(&core_file).unwrap(), old);
    m.set_in_use(None);

    // update: a newer nightly appears
    bot.publish_good("2026-11-18", &new);
    refresh(&m).unwrap();
    let updates = m.updates();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].id, gambatte);
    assert_eq!(updates[0].installed.to_string(), "2026-11-10");
    assert_eq!(updates[0].available.to_string(), "2026-11-18");
    m.set_pinned(&gambatte, true).unwrap();
    assert!(m.updates()[0].pinned);
    m.set_pinned(&gambatte, false).unwrap();

    let installed = install(&m, "gambatte").unwrap();
    assert!(installed.has_previous);
    assert_eq!(fs::read(&core_file).unwrap(), new);
    assert_eq!(
        fs::read(cores_dir(data.path()).join("gambatte_libretro.so.prev")).unwrap(),
        old
    );
    assert!(m.updates().is_empty());
    m.verified_path(&gambatte).unwrap();

    // rollback swaps, twice brings the new one back; both stay verified
    let rolled = m.rollback(&gambatte).unwrap();
    assert!(rolled.has_previous);
    assert_eq!(fs::read(&core_file).unwrap(), old);
    m.verified_path(&gambatte).unwrap();
    m.rollback(&gambatte).unwrap();
    assert_eq!(fs::read(&core_file).unwrap(), new);
    m.verified_path(&gambatte).unwrap();

    // tamper with the file outside the application: never handed to the runtime
    let mut evil = new.clone();
    evil.push(0);
    fs::write(&core_file, &evil).unwrap();
    assert!(matches!(
        m.verified_path(&gambatte),
        Err(CoreError::Tampered { .. })
    ));
    let mut flipped = new.clone();
    flipped[100] ^= 1;
    fs::write(&core_file, &flipped).unwrap();
    assert!(matches!(
        m.verified_path(&gambatte),
        Err(CoreError::Tampered { .. })
    ));
    fs::write(&core_file, b"").unwrap();
    assert!(matches!(
        m.verified_path(&gambatte),
        Err(CoreError::Tampered { .. })
    ));
    // Reinstalling repairs it.
    install(&m, "gambatte").unwrap();
    assert_eq!(fs::read(&core_file).unwrap(), new);
    m.verified_path(&gambatte).unwrap();

    // remove: file, .prev and manifest entry go away
    m.remove(&gambatte).unwrap();
    assert!(!core_file.exists());
    assert!(
        !cores_dir(data.path())
            .join("gambatte_libretro.so.prev")
            .exists()
    );
    assert!(m.installed().is_empty());
    assert!(matches!(
        m.remove(&gambatte),
        Err(CoreError::NotInstalled(_))
    ));
    assert!(matches!(
        m.verified_path(&gambatte),
        Err(CoreError::NotInstalled(_))
    ));
    assert!(tmp_leftovers(data.path()).is_empty());
}

#[test]
fn rollback_without_previous_is_refused() {
    let core = mock_core_bytes(Variant::Default);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    bot.publish_good("2026-11-10", &core);
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();
    assert!(matches!(
        m.rollback(&id("gambatte")),
        Err(CoreError::NoPrevious(_))
    ));
}

#[test]
fn bad_downloads_leave_the_installation_untouched() {
    let old = mock_core_bytes(Variant::Default);
    let new = mock_core_bytes(Variant::Rgb565);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    let gambatte = id("gambatte");
    let core_file = cores_dir(data.path()).join("gambatte_libretro.so");
    bot.publish_good("2026-11-10", &old);
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();

    // Index CRC does not match the zipped core.
    bot.publish(
        "2026-11-18",
        crc32fast::hash(&new) ^ 1,
        "gambatte_libretro.so",
        &new,
    );
    refresh(&m).unwrap();
    let err = install(&m, "gambatte").unwrap_err();
    assert!(matches!(err, CoreError::CrcMismatch { .. }), "{err:?}");

    // The zip holds another file name.
    bot.publish(
        "2026-11-18",
        crc32fast::hash(&new),
        "evil_libretro.so",
        &new,
    );
    refresh(&m).unwrap();
    let err = install(&m, "gambatte").unwrap_err();
    assert!(
        matches!(err, CoreError::Archive(ArchiveError::MissingEntry(_))),
        "{err:?}"
    );

    // The zip is garbage.
    bot.publish_raw("2026-11-18", crc32fast::hash(&new), vec![0x50; 4096]);
    refresh(&m).unwrap();
    let err = install(&m, "gambatte").unwrap_err();
    assert!(matches!(err, CoreError::Archive(_)), "{err:?}");

    let before = fs::read(&core_file).unwrap();
    assert_eq!(before, old);
    m.verified_path(&gambatte).unwrap();
    assert!(
        !cores_dir(data.path())
            .join("gambatte_libretro.so.prev")
            .exists()
    );
    assert!(tmp_leftovers(data.path()).is_empty());
}

#[test]
fn failed_refresh_keeps_the_previous_catalog() {
    let core = mock_core_bytes(Variant::Default);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    bot.publish_good("2026-11-10", &core);
    refresh(&m).unwrap();

    // Empty index, then a missing info.zip: both are errors and the catalog stays.
    bot.rt.block_on(bot.server.reset());
    bot.mount(
        Mock::given(path(INDEX)).respond_with(ResponseTemplate::new(200).set_body_string("")),
    );
    assert!(refresh(&m).is_err());
    assert_eq!(m.catalog().unwrap().cores.len(), 1);

    bot.rt.block_on(bot.server.reset());
    bot.mount(
        Mock::given(path(INDEX)).respond_with(
            ResponseTemplate::new(200)
                .set_body_string("2026-11-10 00000001 gambatte_libretro.so.zip\n"),
        ),
    );
    bot.mount(Mock::given(path(INFO_ZIP)).respond_with(ResponseTemplate::new(404)));
    let err = refresh(&m).unwrap_err();
    assert!(
        matches!(err, CoreError::Asset(AssetError::Http { status: 404 })),
        "{err:?}"
    );
    assert_eq!(
        m.catalog().unwrap().cores[&id("gambatte")].crc32,
        crc32fast::hash(&core)
    );
    assert!(tmp_leftovers(data.path()).is_empty());
}

#[test]
fn cancelled_install_writes_nothing() {
    let core = mock_core_bytes(Variant::Default);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    bot.publish_good("2026-11-10", &core);
    refresh(&m).unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let err = m
        .install(&id("gambatte"), &mut |_| {}, &cancel)
        .unwrap_err();
    assert!(
        matches!(err, CoreError::Asset(AssetError::Cancelled)),
        "{err:?}"
    );
    assert!(m.installed().is_empty());
    assert!(tmp_leftovers(data.path()).is_empty());
}

#[test]
fn manual_core_is_listed_and_usable_without_verification() {
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    let file = cores_dir(data.path()).join("custom_libretro.so");
    fs::write(&file, b"whatever").unwrap();
    let list = m.installed();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].origin, Origin::Manual);
    assert_eq!(m.verified_path(&id("custom")).unwrap(), file);
}

#[test]
fn install_through_a_background_job() {
    let core = mock_core_bytes(Variant::Default);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = Arc::new(open(&bot, data.path()));
    bot.publish_good("2026-11-10", &core);
    refresh(&m).unwrap();

    let worker = Arc::clone(&m);
    let mut job = Job::spawn("install-test", Arc::new(|| {}), move |progress, cancel| {
        worker.install(&id("gambatte"), progress, cancel)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut result = None;
    while result.is_none() {
        assert!(Instant::now() < deadline, "job did not finish");
        for event in job.poll() {
            if let JobEvent::Done(r) = event {
                result = Some(r);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(result.unwrap().is_ok());
    m.verified_path(&id("gambatte")).unwrap();
}

fn requests_to(bot: &Buildbot, route: &str) -> usize {
    bot.rt
        .block_on(bot.server.received_requests())
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == route)
        .count()
}

#[test]
fn outdated_cached_index_is_renewed_after_a_crc_mismatch() {
    let old = mock_core_bytes(Variant::Default);
    let new = mock_core_bytes(Variant::Rgb565);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    // Cached index describes the previous build while the zip already holds the new one.
    bot.publish(
        "2026-11-10",
        crc32fast::hash(&old),
        "gambatte_libretro.so",
        &new,
    );
    refresh(&m).unwrap();
    bot.publish_good("2026-11-20", &new);
    install(&m, "gambatte").unwrap();
    assert_eq!(
        fs::read(cores_dir(data.path()).join("gambatte_libretro.so")).unwrap(),
        new
    );
    assert_eq!(requests_to(&bot, GAMBATTE_ZIP), 1);
    assert!(tmp_leftovers(data.path()).is_empty());
}

#[test]
fn fresh_index_with_a_corrupt_zip_installs_nothing() {
    let new = mock_core_bytes(Variant::Rgb565);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    bot.publish(
        "2026-11-20",
        crc32fast::hash(&new) ^ 1,
        "gambatte_libretro.so",
        &new,
    );
    refresh(&m).unwrap();
    let err = install(&m, "gambatte").unwrap_err();
    assert!(matches!(err, CoreError::CrcMismatch { .. }), "{err:?}");
    assert!(
        err.to_string()
            .contains("même après mise à jour du catalogue")
    );
    // The renewed index is unchanged: no second download.
    assert_eq!(requests_to(&bot, GAMBATTE_ZIP), 1);
    assert_eq!(requests_to(&bot, INDEX), 2);
    assert!(!cores_dir(data.path()).join("gambatte_libretro.so").exists());
    assert!(tmp_leftovers(data.path()).is_empty());
}

#[test]
fn catalog_older_than_the_limit_is_renewed_before_downloading() {
    let old = mock_core_bytes(Variant::Default);
    let new = mock_core_bytes(Variant::Rgb565);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    {
        let m = open(&bot, data.path());
        bot.publish_good("2026-11-10", &old);
        refresh(&m).unwrap();
    }
    let manifest = cores_dir(data.path()).join("manifest.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    let aged: Vec<String> = text
        .lines()
        .map(|l| {
            if l.starts_with("index_fetched_unix") {
                "index_fetched_unix = 1000".to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    assert_ne!(
        aged.join("\n"),
        text.trim_end(),
        "manifest has no fetch date"
    );
    fs::write(&manifest, aged.join("\n")).unwrap();

    let m = open(&bot, data.path());
    assert_eq!(m.catalog().unwrap().fetched_unix, 1000);
    bot.publish_good("2026-11-20", &new);
    install(&m, "gambatte").unwrap();
    assert_eq!(requests_to(&bot, GAMBATTE_ZIP), 1);
    assert!(m.catalog().unwrap().fetched_unix > 1000);
}

#[test]
fn offline_with_an_old_catalog_lets_the_crc_check_decide() {
    let new = mock_core_bytes(Variant::Rgb565);
    assert!(
        emu_assets::cores::CATALOG_MAX_AGE >= Duration::from_secs(3600),
        "documented constant"
    );
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    bot.publish_good("2026-11-20", &new);
    refresh(&m).unwrap();
    // Index and info.zip vanish: the renewal fails, the verified install still works.
    bot.rt.block_on(bot.server.reset());
    bot.mount(Mock::given(path(GAMBATTE_ZIP)).respond_with(
        ResponseTemplate::new(200).set_body_bytes(zip_of(&[("gambatte_libretro.so", &new)])),
    ));
    install(&m, "gambatte").unwrap();
}

const FUTURE: &str = "Fri, 01 Jan 2100 00:00:00 GMT";
const PAST: &str = "Mon, 01 Jan 2001 00:00:00 GMT";

fn install_with_wrong_crc(modified: Option<&str>) -> CoreError {
    let new = mock_core_bytes(Variant::Rgb565);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    let wrong = crc32fast::hash(&new) ^ 1;
    match modified {
        Some(lm) => bot.publish_with_last_modified(wrong, &new, lm),
        None => bot.publish("2026-11-20", wrong, "gambatte_libretro.so", &new),
    }
    refresh(&m).unwrap();
    let err = install(&m, "gambatte").unwrap_err();
    assert!(m.installed().is_empty());
    assert!(!cores_dir(data.path()).join("gambatte_libretro.so").exists());
    assert!(tmp_leftovers(data.path()).is_empty());
    err
}

#[test]
fn wrong_crc_with_a_zip_newer_than_the_index_is_buildbot_updating() {
    let err = install_with_wrong_crc(Some(FUTURE));
    assert!(matches!(err, CoreError::BuildbotUpdating(_)), "{err:?}");
    assert!(err.to_string().contains("en cours de mise à jour"));
    assert!(!err.to_string().contains("CRC"));
}

#[test]
fn wrong_crc_with_an_old_zip_stays_a_crc_error() {
    let err = install_with_wrong_crc(Some(PAST));
    assert!(matches!(err, CoreError::CrcMismatch { .. }), "{err:?}");
}

#[test]
fn wrong_crc_without_last_modified_stays_a_crc_error() {
    let err = install_with_wrong_crc(None);
    assert!(matches!(err, CoreError::CrcMismatch { .. }), "{err:?}");
}

/// Index dated 2026-11-20, wrong CRC, zip served with `modified`.
fn install_dated(modified: &str, attempts: usize) -> Vec<CoreError> {
    let new = mock_core_bytes(Variant::Rgb565);
    let bot = Buildbot::start();
    let data = tempfile::tempdir().unwrap();
    let m = open(&bot, data.path());
    bot.publish_with_last_modified(crc32fast::hash(&new) ^ 1, &new, modified);
    refresh(&m).unwrap();
    (0..attempts)
        .map(|_| install(&m, "gambatte").unwrap_err())
        .collect()
}

#[test]
fn buildbot_updating_is_stable_across_attempts() {
    for err in install_dated("Sat, 21 Nov 2026 07:30:00 GMT", 2) {
        assert!(matches!(err, CoreError::BuildbotUpdating(_)), "{err:?}");
    }
}

#[test]
fn zip_modified_the_day_after_the_index_is_buildbot_updating() {
    let errs = install_dated("Sat, 21 Nov 2026 00:00:00 GMT", 1);
    assert!(
        matches!(errs[0], CoreError::BuildbotUpdating(_)),
        "{errs:?}"
    );
}

#[test]
fn zip_modified_on_the_index_day_stays_a_crc_error() {
    for lm in [
        "Fri, 20 Nov 2026 12:00:00 GMT",
        "Fri, 20 Nov 2026 23:59:59 GMT",
    ] {
        for err in install_dated(lm, 2) {
            assert!(
                matches!(err, CoreError::CrcMismatch { .. }),
                "{lm}: {err:?}"
            );
        }
    }
}
