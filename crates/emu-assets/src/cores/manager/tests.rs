//! wiremock + tempdir only. The blocking client must not run inside the tokio runtime: each test owns a runtime
//! for wiremock only (same pattern as `http/tests.rs`).

use std::io::Cursor;
use std::sync::Arc;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::write::SimpleFileOptions;

use super::*;
use crate::http::HttpConfig;

const P: Platform = Platform::LinuxX86_64;

struct Server {
    server: MockServer,
    rt: tokio::runtime::Runtime,
}

impl Server {
    fn start() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let server = rt.block_on(MockServer::start());
        Self { server, rt }
    }

    fn mount(&self, mock: Mock) {
        self.rt.block_on(mock.mount(&self.server));
    }

    fn reset(&self) {
        self.rt.block_on(self.server.reset());
    }

    fn uri(&self) -> String {
        self.server.uri()
    }
}

fn id(s: &str) -> CoreId {
    CoreId::new(s).unwrap()
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

const INFO: &str = "display_name = \"Nintendo - Game Boy / Color (Gambatte)\"\n\
    supported_extensions = \"gb|gbc|dmg\"\nlicense = \"GPLv2\"\n\
    database = \"Nintendo - Game Boy|Nintendo - Game Boy Color\"\n\
    firmware0_path = \"gb_bios.bin\"\nfirmware0_opt = \"true\"\n";

struct Fixture {
    server: Server,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Self {
            server: Server::start(),
            dir: tempfile::tempdir().unwrap(),
        };
        f.serve_info();
        f
    }

    fn dirs(&self) -> CoreDirs {
        CoreDirs::under(self.dir.path(), P)
    }

    fn cores(&self) -> PathBuf {
        self.dirs().cores
    }

    fn open(&self) -> CoreManager {
        let http = HttpClient::new(HttpConfig {
            allow_loopback_http: true,
            timeout: Duration::from_secs(10),
            ..HttpConfig::default()
        })
        .unwrap();
        let source = BuildbotSource::custom(&self.server.uri()).unwrap();
        CoreManager::open(self.dirs(), P, source, http).unwrap()
    }

    fn serve_info(&self) {
        let bundle = zip_of(&[("gambatte_libretro.info", INFO.as_bytes())]);
        self.server.mount(
            Mock::given(method("GET"))
                .and(path("/assets/frontend/info.zip"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(bundle)),
        );
    }

    /// Index listing `gambatte` with `index_crc`, and its zip containing `core`.
    fn serve_core(&self, date: &str, index_crc: u32, core: &[u8]) {
        let index = format!(
            "{date} {index_crc:08x} gambatte_libretro.so.zip\n2026-11-16 00000000 other_libretro.so.zip\n"
        );
        self.server.mount(
            Mock::given(path("/nightly/linux/x86_64/latest/.index-extended"))
                .respond_with(ResponseTemplate::new(200).set_body_string(index)),
        );
        self.server.mount(
            Mock::given(path(
                "/nightly/linux/x86_64/latest/gambatte_libretro.so.zip",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(zip_of(&[("gambatte_libretro.so", core)])),
            ),
        );
    }

    fn serve_good(&self, date: &str, core: &[u8]) {
        self.serve_core(date, crc32fast::hash(core), core);
    }

    fn leftovers(&self) -> Vec<String> {
        let mut found = Vec::new();
        for dir in [
            self.cores(),
            self.cores().join(INFO_DIR),
            self.cores().join(CACHE_DIR),
        ] {
            for e in fs::read_dir(dir).unwrap() {
                let name = e.unwrap().file_name().into_string().unwrap();
                if name.contains(".tmp-") {
                    found.push(name);
                }
            }
        }
        found
    }
}

fn refresh(m: &CoreManager) -> Result<RemoteCatalog, CoreError> {
    m.refresh(&mut |_| {}, &CancelToken::new())
}

fn install(m: &CoreManager, core: &str) -> Result<InstalledCore, CoreError> {
    m.install(&id(core), &mut |_| {}, &CancelToken::new())
}

#[test]
fn manager_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<CoreManager>();
}

#[test]
fn sources() {
    assert!(BuildbotSource::official().is_official());
    assert!(
        BuildbotSource::custom("https://buildbot.libretro.com/")
            .unwrap()
            .is_official()
    );
    for bad in [
        "http://example.com",
        "ftp://x",
        "https://user:pw@example.com",
        "https://example.com/?a=b",
        "file:///etc",
        "not a url",
    ] {
        assert!(
            matches!(BuildbotSource::custom(bad), Err(AssetError::UrlRefused)),
            "{bad}"
        );
    }
    let s = BuildbotSource::official();
    assert_eq!(
        s.core_url(Platform::MacosX86_64, &id("gambatte")),
        "https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/gambatte_libretro.dylib.zip"
    );
    assert_eq!(
        s.index_url(Platform::WindowsX86_64),
        "https://buildbot.libretro.com/nightly/windows/x86_64/latest/.index-extended"
    );
    assert_eq!(
        s.info_bundle_url(),
        "https://buildbot.libretro.com/assets/frontend/info.zip"
    );
    let dirs = CoreDirs::under(Path::new("/d"), Platform::WindowsX86_64);
    assert_eq!(dirs.cores, Path::new("/d/cores/windows-x86_64"));
    assert_eq!(dirs.system, Path::new("/d/system"));
}

#[test]
fn refresh_install_list_and_verify() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    assert!(m.catalog().is_none());
    assert!(matches!(install(&m, "gambatte"), Err(CoreError::NoCatalog)));

    let catalog = refresh(&m).unwrap();
    assert_eq!(catalog.cores.len(), 2);
    assert_eq!(
        m.info(&id("gambatte")).unwrap().license.as_deref(),
        Some("GPLv2")
    );
    assert!(matches!(
        install(&m, "nope"),
        Err(CoreError::NotAvailable(_))
    ));

    let installed = install(&m, "gambatte").unwrap();
    assert_eq!(installed.path, f.cores().join("gambatte_libretro.so"));
    assert!(!installed.has_previous);
    let Origin::Buildbot {
        sha256,
        size,
        pinned,
        date,
        ..
    } = installed.origin
    else {
        panic!("not managed");
    };
    assert_eq!(sha256, <[u8; 32]>::from(Sha256::digest(b"core v1")));
    assert_eq!(
        (size, pinned, date.to_string().as_str()),
        (7, false, "2026-11-16")
    );
    assert_eq!(fs::read(&installed.path).unwrap(), b"core v1");
    assert_eq!(m.installed(), std::slice::from_ref(&installed));
    assert_eq!(m.verified_path(&id("gambatte")).unwrap(), installed.path);
    assert!(m.updates().is_empty());
    assert!(f.leftovers().is_empty());

    // Everything survives a reopen (manifest, cached index, .info).
    let manifest = fs::read_to_string(f.cores().join(MANIFEST_FILE)).unwrap();
    assert!(manifest.contains("version = 1"), "{manifest}");
    assert!(
        manifest.contains("file = \"gambatte_libretro.so\""),
        "{manifest}"
    );
    let m2 = f.open();
    assert_eq!(m2.installed(), [installed]);
    assert_eq!(m2.catalog().unwrap().cores.len(), 2);
    assert_eq!(m2.catalog().unwrap().fetched_unix, catalog.fetched_unix);
    assert!(m2.info(&id("gambatte")).is_some());
    assert_eq!(
        m2.firmware_status(&id("gambatte"))[0].state,
        super::super::FirmwareState::Missing
    );
}

#[test]
fn update_keeps_previous_then_rollback() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();
    m.set_pinned(&id("gambatte"), true).unwrap();

    f.server.reset();
    f.serve_info();
    f.serve_good("2026-11-18", b"core v2");
    refresh(&m).unwrap();
    let updates = m.updates();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].installed.to_string(), "2026-11-16");
    assert_eq!(updates[0].available.to_string(), "2026-11-18");
    assert!(updates[0].pinned);

    let v2 = install(&m, "gambatte").unwrap();
    assert!(v2.has_previous);
    assert!(matches!(v2.origin, Origin::Buildbot { pinned: true, .. }));
    assert_eq!(fs::read(&v2.path).unwrap(), b"core v2");
    assert!(m.updates().is_empty());

    let v1 = m.rollback(&id("gambatte")).unwrap();
    assert_eq!(fs::read(&v1.path).unwrap(), b"core v1");
    assert!(matches!(v1.origin, Origin::Buildbot { date, .. } if date.to_string() == "2026-11-16"));
    assert!(v1.has_previous);
    m.verified_path(&id("gambatte")).unwrap();
    assert_eq!(m.updates().len(), 1);

    // Rolling back again returns to v2, still verified.
    m.rollback(&id("gambatte")).unwrap();
    assert_eq!(fs::read(&v1.path).unwrap(), b"core v2");
    m.verified_path(&id("gambatte")).unwrap();
    // And the state survives a reopen.
    let m2 = f.open();
    m2.verified_path(&id("gambatte")).unwrap();
    assert!(m2.installed()[0].has_previous);
    assert!(f.leftovers().is_empty());
}

#[test]
fn crc_mismatch_writes_nothing() {
    let f = Fixture::new();
    f.serve_core("2026-11-16", 0x1234_5678, b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    let err = install(&m, "gambatte").unwrap_err();
    assert!(
        matches!(
            err,
            CoreError::CrcMismatch {
                expected: 0x1234_5678,
                ..
            }
        ),
        "{err}"
    );
    assert!(m.installed().is_empty());
    assert!(
        !f.cores().join(MANIFEST_FILE).exists()
            || !fs::read_to_string(f.cores().join(MANIFEST_FILE))
                .unwrap()
                .contains("gambatte")
    );
    assert!(f.leftovers().is_empty());
}

#[test]
fn failed_or_cancelled_update_keeps_previous_install() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();

    // New index, but the zip is not a zip.
    f.server.reset();
    f.serve_info();
    f.server.mount(
        Mock::given(path("/nightly/linux/x86_64/latest/.index-extended")).respond_with(
            ResponseTemplate::new(200)
                .set_body_string("2026-11-18 00000001 gambatte_libretro.so.zip\n"),
        ),
    );
    f.server.mount(
        Mock::given(path(
            "/nightly/linux/x86_64/latest/gambatte_libretro.so.zip",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"truncated".to_vec())),
    );
    refresh(&m).unwrap();
    assert!(matches!(
        install(&m, "gambatte"),
        Err(CoreError::Archive(_))
    ));

    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        m.install(&id("gambatte"), &mut |_| {}, &cancel),
        Err(CoreError::Asset(AssetError::Cancelled))
    ));

    // Cancelled in the middle of the body.
    f.server.reset();
    f.server.mount(
        Mock::given(path(
            "/nightly/linux/x86_64/latest/gambatte_libretro.so.zip",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 512 * 1024])),
    );
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let err = m
        .install(
            &id("gambatte"),
            &mut |p| {
                if p.done > 0 {
                    token.cancel();
                }
            },
            &cancel,
        )
        .unwrap_err();
    assert!(
        matches!(err, CoreError::Asset(AssetError::Cancelled)),
        "{err}"
    );

    let installed = m.installed();
    assert_eq!(installed.len(), 1);
    assert!(!installed[0].has_previous);
    assert_eq!(fs::read(&installed[0].path).unwrap(), b"core v1");
    m.verified_path(&id("gambatte")).unwrap();
    assert!(f.leftovers().is_empty());
}

#[test]
fn transient_errors_are_retried() {
    let f = Fixture::new();
    f.server.mount(
        Mock::given(path("/nightly/linux/x86_64/latest/.index-extended"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .with_priority(1),
    );
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();

    f.server.reset();
    f.server.mount(
        Mock::given(path(
            "/nightly/linux/x86_64/latest/gambatte_libretro.so.zip",
        ))
        .respond_with(ResponseTemplate::new(404)),
    );
    assert!(matches!(
        install(&m, "gambatte"),
        Err(CoreError::Asset(AssetError::Http { status: 404 }))
    ));
}

#[test]
fn bad_index_keeps_previous_catalog() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    let before = refresh(&m).unwrap();
    f.server.reset();
    f.serve_info();
    f.server.mount(
        Mock::given(path("/nightly/linux/x86_64/latest/.index-extended"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>oops</html>")),
    );
    assert!(matches!(
        refresh(&m),
        Err(CoreError::Asset(AssetError::InvalidResponse(_)))
    ));
    assert_eq!(m.catalog(), Some(before));
    assert!(m.info(&id("gambatte")).is_some());
}

#[test]
fn modified_core_is_never_loaded() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    let installed = install(&m, "gambatte").unwrap();
    fs::write(&installed.path, b"core v!").unwrap();
    assert!(matches!(
        m.verified_path(&id("gambatte")),
        Err(CoreError::Tampered { .. })
    ));
    fs::write(&installed.path, b"core v1 + more").unwrap();
    assert!(matches!(
        m.verified_path(&id("gambatte")),
        Err(CoreError::Tampered { .. })
    ));
    fs::remove_file(&installed.path).unwrap();
    assert!(matches!(
        m.verified_path(&id("gambatte")),
        Err(CoreError::NotInstalled(_))
    ));
}

#[test]
fn core_in_use_is_never_touched() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();
    install(&m, "gambatte").unwrap(); // creates a .prev
    m.set_in_use(Some(id("gambatte")));
    assert!(matches!(install(&m, "gambatte"), Err(CoreError::InUse(_))));
    assert!(matches!(
        m.remove(&id("gambatte")),
        Err(CoreError::InUse(_))
    ));
    assert!(matches!(
        m.rollback(&id("gambatte")),
        Err(CoreError::InUse(_))
    ));
    m.verified_path(&id("gambatte")).unwrap();
    m.set_in_use(None);
    m.remove(&id("gambatte")).unwrap();
    assert!(m.installed().is_empty());
    assert!(!f.cores().join("gambatte_libretro.so.prev").exists());
    assert!(matches!(
        m.remove(&id("gambatte")),
        Err(CoreError::NotInstalled(_))
    ));
    assert!(matches!(
        m.set_pinned(&id("gambatte"), true),
        Err(CoreError::NotInstalled(_))
    ));
}

#[test]
fn in_use_set_during_download_wins() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();
    let err = m
        .install(
            &id("gambatte"),
            &mut |_| m.set_in_use(Some(id("gambatte"))),
            &CancelToken::new(),
        )
        .unwrap_err();
    assert!(matches!(err, CoreError::InUse(_)));
    assert!(!m.installed()[0].has_previous);
    assert!(f.leftovers().is_empty());
}

#[test]
fn install_rolls_back_when_the_manifest_cannot_be_written() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    // A directory in place of manifest.toml: unreadable (so a new one is started) and impossible to replace.
    let manifest = f.cores().join(MANIFEST_FILE);
    let err = m
        .install(
            &id("gambatte"),
            &mut |_| {
                let _ = fs::remove_file(&manifest);
                let _ = fs::create_dir(&manifest);
            },
            &CancelToken::new(),
        )
        .unwrap_err();
    assert!(!matches!(err, CoreError::InUse(_)), "{err}");
    assert!(manifest.is_dir(), "the failure must come from the manifest");
    assert!(!f.cores().join("gambatte_libretro.so").exists());
    assert!(!f.cores().join("gambatte_libretro.so.prev").exists());
    assert!(f.leftovers().is_empty());
}

#[test]
fn concurrent_installs_are_busy() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = Arc::new(f.open());
    refresh(&m).unwrap();
    f.server.reset();
    let zip = zip_of(&[("gambatte_libretro.so", b"core v1")]);
    f.server.mount(
        Mock::given(path(
            "/nightly/linux/x86_64/latest/gambatte_libretro.so.zip",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip)),
    );
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (checked_tx, checked_rx) = std::sync::mpsc::channel::<()>();
    let m2 = m.clone();
    let first = std::thread::spawn(move || {
        let mut once = Some((started_tx, checked_rx));
        m2.install(
            &id("gambatte"),
            &mut |_| {
                // Holds the first install inside its download until the main thread has checked.
                if let Some((started, checked)) = once.take() {
                    started.send(()).unwrap();
                    checked.recv_timeout(Duration::from_secs(10)).unwrap();
                }
            },
            &CancelToken::new(),
        )
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(install(&m, "gambatte"), Err(CoreError::Busy(_))));
    assert!(matches!(m.remove(&id("gambatte")), Err(CoreError::Busy(_))));
    assert!(matches!(
        m.rollback(&id("gambatte")),
        Err(CoreError::Busy(_))
    ));
    checked_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    // Not busy any more.
    assert!(matches!(
        m.rollback(&id("gambatte")),
        Err(CoreError::NoPrevious(_))
    ));
}

#[test]
fn corrupt_manifest_means_manual_and_is_not_rewritten() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();
    drop(m);

    let manifest = f.cores().join(MANIFEST_FILE);
    fs::write(&manifest, "version = 1\n[cores.gambatte\n").unwrap();
    let m = f.open();
    assert_eq!(m.installed()[0].origin, Origin::Manual);
    assert!(m.updates().is_empty());
    m.verified_path(&id("gambatte")).unwrap();
    refresh(&m).unwrap();
    assert_eq!(
        fs::read_to_string(&manifest).unwrap(),
        "version = 1\n[cores.gambatte\n"
    );
    assert!(matches!(
        m.set_pinned(&id("gambatte"), true),
        Err(CoreError::NotInstalled(_))
    ));

    // An install is a user action: a new manifest is written.
    let installed = install(&m, "gambatte").unwrap();
    assert!(matches!(installed.origin, Origin::Buildbot { .. }));
    assert!(fs::read_to_string(&manifest).unwrap().contains("sha256"));
}

#[test]
fn manifest_entry_for_another_file_is_ignored() {
    let f = Fixture::new();
    let m = f.open();
    drop(m);
    fs::write(f.cores().join("gambatte_libretro.so"), b"x").unwrap();
    let sha = hex(&Sha256::digest(b"x"));
    fs::write(
        f.cores().join(MANIFEST_FILE),
        format!(
            "version = 1\n[cores.gambatte]\nfile = \"../evil.so\"\nurl = \"u\"\ndate = \"2026-11-16\"\n\
             crc32 = \"00000000\"\nsha256 = \"{sha}\"\nsize = 1\ninstalled_unix = 0\n"
        ),
    )
    .unwrap();
    assert_eq!(f.open().installed()[0].origin, Origin::Manual);
}

#[test]
fn manual_core_is_listed_and_usable() {
    let f = Fixture::new();
    let m = f.open();
    fs::write(f.cores().join("mesen_libretro.so"), b"manual").unwrap();
    fs::write(f.cores().join("readme.txt"), b"").unwrap();
    fs::write(f.cores().join("x_libretro.dll"), b"").unwrap();
    let installed = m.installed();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, id("mesen"));
    assert_eq!(installed[0].origin, Origin::Manual);
    assert_eq!(m.verified_path(&id("mesen")).unwrap(), installed[0].path);
    assert!(matches!(
        m.rollback(&id("mesen")),
        Err(CoreError::NoPrevious(_))
    ));
    m.remove(&id("mesen")).unwrap();
    assert!(m.installed().is_empty());
}

#[test]
fn open_repairs_an_interrupted_run() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();
    drop(m);

    let cores = f.cores();
    // Cut between the two renames of an update: the current file is only in `.prev`.
    fs::rename(
        cores.join("gambatte_libretro.so"),
        cores.join("gambatte_libretro.so.prev"),
    )
    .unwrap();
    for temp in [
        cores.join(".gambatte_libretro.so.tmp-1-0"),
        cores.join("info").join(".x.tmp-2-0"),
        cores.join(".cache").join(".index-extended.tmp-3-0"),
    ] {
        fs::write(&temp, b"partial").unwrap();
        set_age(&temp, ORPHAN_AGE + Duration::from_secs(60));
    }
    // Possibly being written by another process right now: kept.
    fs::write(cores.join(".mesen_libretro.so.tmp-4-0"), b"partial").unwrap();

    let m = f.open();
    assert_eq!(f.leftovers(), [".mesen_libretro.so.tmp-4-0"]);
    fs::remove_file(cores.join(".mesen_libretro.so.tmp-4-0")).unwrap();
    let installed = m.installed();
    assert_eq!(installed.len(), 1);
    assert!(!installed[0].has_previous);
    m.verified_path(&id("gambatte")).unwrap();
    // Idempotent.
    let m = f.open();
    assert_eq!(m.installed(), installed);
}

fn set_age(path: &Path, age: Duration) {
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

/// Index with `gambatte` and `mesen`, both downloadable.
fn serve_two(f: &Fixture, gambatte: &[u8], mesen: &[u8]) {
    let index = format!(
        "2026-11-16 {:08x} gambatte_libretro.so.zip\n2026-11-17 {:08x} mesen_libretro.so.zip\n",
        crc32fast::hash(gambatte),
        crc32fast::hash(mesen)
    );
    f.server.mount(
        Mock::given(path("/nightly/linux/x86_64/latest/.index-extended"))
            .respond_with(ResponseTemplate::new(200).set_body_string(index)),
    );
    for (name, data) in [("gambatte", gambatte), ("mesen", mesen)] {
        f.server.mount(
            Mock::given(path(format!(
                "/nightly/linux/x86_64/latest/{name}_libretro.so.zip"
            )))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(zip_of(&[(&format!("{name}_libretro.so"), data)])),
            ),
        );
    }
}

// Two managers on one directory stand for two processes: the directory lock is taken through separate file
// handles, which conflict within a process as they do between processes.

#[test]
fn second_process_does_not_drop_entries_of_the_first() {
    let f = Fixture::new();
    serve_two(&f, b"gambatte v1", b"mesen v1");
    let a = f.open();
    let b = f.open();
    refresh(&a).unwrap();
    refresh(&b).unwrap();

    install(&b, "mesen").unwrap();
    // `a` never saw mesen's install in memory: its write must keep it.
    install(&a, "gambatte").unwrap();
    a.set_pinned(&id("mesen"), true).unwrap();
    b.set_pinned(&id("gambatte"), true).unwrap();

    for m in [&a, &b, &f.open()] {
        let installed = m.installed();
        assert_eq!(installed.len(), 2);
        for core in &installed {
            assert!(
                matches!(core.origin, Origin::Buildbot { pinned: true, .. }),
                "{core:?}"
            );
        }
    }
    // Still verified from `a`, not loaded as a manual core.
    fs::write(f.cores().join("mesen_libretro.so"), b"mesen v!").unwrap();
    assert!(matches!(
        a.verified_path(&id("mesen")),
        Err(CoreError::Tampered { .. })
    ));
    a.verified_path(&id("gambatte")).unwrap();

    b.remove(&id("gambatte")).unwrap();
    assert_eq!(a.installed().len(), 1);
    assert!(a.updates().is_empty());
}

#[test]
fn concurrent_installs_from_two_processes() {
    let f = Fixture::new();
    serve_two(&f, b"gambatte v1", b"mesen v1");
    let a = Arc::new(f.open());
    let b = Arc::new(f.open());
    refresh(&a).unwrap();
    refresh(&b).unwrap();
    let threads: Vec<_> = [
        (&a, "gambatte"),
        (&b, "gambatte"),
        (&a, "mesen"),
        (&b, "mesen"),
    ]
    .into_iter()
    .map(|(m, core)| {
        let m = Arc::clone(m);
        std::thread::spawn(move || install(&m, core).map(|_| ()))
    })
    .collect();
    for t in threads {
        t.join().unwrap().unwrap();
    }
    for m in [&*a, &*b, &f.open()] {
        let installed = m.installed();
        assert_eq!(installed.len(), 2);
        for core in installed {
            assert!(matches!(core.origin, Origin::Buildbot { .. }));
            m.verified_path(&core.id).unwrap();
        }
    }
    assert!(f.leftovers().is_empty());
}

#[test]
fn open_waits_for_a_section_of_another_process() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();
    install(&m, "gambatte").unwrap();

    let cores = f.cores();
    let current = cores.join("gambatte_libretro.so");
    let prev = cores.join("gambatte_libretro.so.prev");
    let swap_temp = cores.join(".gambatte_libretro.so.tmp-1-0");
    // Another process in the middle of a rollback: `.prev` moved aside, current moved to `.prev`.
    let held = DirLock::acquire(&cores).unwrap();
    fs::rename(&prev, &swap_temp).unwrap();
    fs::rename(&current, &prev).unwrap();
    set_age(&swap_temp, ORPHAN_AGE * 2);

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let opener = {
        let dirs = f.dirs();
        let source = BuildbotSource::custom(&f.server.uri()).unwrap();
        std::thread::spawn(move || {
            let http = HttpClient::new(HttpConfig::default()).unwrap();
            let m = CoreManager::open(dirs, P, source, http).unwrap();
            done_tx.send(()).unwrap();
            m
        })
    };
    assert!(done_rx.recv_timeout(Duration::from_millis(300)).is_err());
    // Nothing repaired nor removed while the other process holds the lock.
    assert!(swap_temp.exists() && prev.exists() && !current.exists());
    fs::rename(&swap_temp, &current).unwrap();
    drop(held);

    let opened = opener.join().unwrap();
    assert_eq!(opened.installed().len(), 1);
    assert!(opened.installed()[0].has_previous);
    opened.verified_path(&id("gambatte")).unwrap();
}

#[test]
fn rollback_of_a_tampered_core_is_refused() {
    let f = Fixture::new();
    f.serve_good("2026-11-16", b"core v1");
    let m = f.open();
    refresh(&m).unwrap();
    install(&m, "gambatte").unwrap();
    install(&m, "gambatte").unwrap();
    // As after a crash between the renames of an install and its manifest write.
    let path = f.cores().join("gambatte_libretro.so");
    fs::write(&path, b"core v2").unwrap();
    assert!(matches!(
        m.rollback(&id("gambatte")),
        Err(CoreError::Tampered { .. })
    ));
    assert_eq!(fs::read(&path).unwrap(), b"core v2");
    assert_eq!(
        fs::read(f.cores().join("gambatte_libretro.so.prev")).unwrap(),
        b"core v1"
    );
    // Reinstalling repairs it.
    install(&m, "gambatte").unwrap();
    m.verified_path(&id("gambatte")).unwrap();
}

#[test]
fn lock_errors_meaning_unsupported() {
    assert!(lock_unsupported(&io::Error::from(
        io::ErrorKind::Unsupported
    )));
    #[cfg(unix)]
    assert!(lock_unsupported(&io::Error::from_raw_os_error(
        libc::ENOLCK
    )));
    assert!(!lock_unsupported(&io::Error::from(
        io::ErrorKind::PermissionDenied
    )));
}
