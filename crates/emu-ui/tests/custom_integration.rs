//! 8.0 custom systems, integration (design 8.0 section 4, lot PQ2), simulated buildbot and DAT server (wiremock):
//! the whole way from the core index to a scanned game, and a system without a libretro name that makes no request
//! and writes no DAT nor cover. Each test publishes its own catalog and has its own data folder; one at a time.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use emu_ui::assets::cores::{BuildbotSource, CoreDirs, CoreId, CoreManager, Platform, RenderNeed};
use emu_ui::assets::http::{CancelToken, HttpClient, HttpConfig};
use emu_ui::library_db::{
    CustomSystemDraft, DatSource, DefinitionOrigin, Library, ScanOptions, cartridge_extensions,
};
use emu_ui::test_support::{DialogKind, LibraryHarness};
use emu_ui::{Addability, Config, DataDirs};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::write::SimpleFileOptions;

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const WAIT: Duration = Duration::from_secs(20);

struct Root {
    _tmp: tempfile::TempDir,
    dirs: DataDirs,
}

fn root() -> Root {
    let tmp = tempfile::tempdir().unwrap();
    // `/var` is a link on macOS: the data folder is compared with canonical paths.
    let base = tmp.path().canonicalize().unwrap();
    let dirs = DataDirs::under(&base);
    dirs.create_all().unwrap();
    Root { _tmp: tmp, dirs }
}

fn mock_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
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

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// The path of a URL of the mock server.
fn url_path(server: &MockServer, url: &str) -> String {
    url.strip_prefix(&server.uri()).unwrap().to_owned()
}

fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            found.extend(all_files(&p));
        } else {
            found.push(p);
        }
    }
    found
}

fn scan(root: &Root) {
    let (mut db, _) = Library::open(&root.dirs.library_db).unwrap();
    let options = ScanOptions {
        roots: vec![root.dirs.root.join("roms")],
        ..ScanOptions::default()
    };
    db.scan(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
}

fn draft(
    id: &str,
    name: &str,
    database: Option<&str>,
    extensions: Vec<String>,
) -> CustomSystemDraft {
    CustomSystemDraft {
        id: id.into(),
        name: name.into(),
        database: database.map(str::to_owned),
        extensions,
        dats: vec![DatSource::NoIntro],
        screenscraper: None,
        igdb: None,
        cores: vec!["zzfic".into()],
        ignored_firmware: Vec::new(),
        origin: DefinitionOrigin {
            core: "zzfic".into(),
            info_name: None,
            created_by: "8.0.0".into(),
            created_unix: 1_764_028_800,
        },
    }
}

#[test]
fn a_system_without_a_libretro_name_makes_no_request_and_writes_no_dat_nor_cover() {
    let _guard = lock();
    let root = root();
    let rt = mock_runtime();
    let server = rt.block_on(MockServer::start());
    // Everything answers 200: any request would be noticed by the log, not by a failure.
    rt.block_on(
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0_u8; 16]))
            .mount(&server),
    );

    let mut h = LibraryHarness::open(root.dirs.clone(), Config::default());
    h.add_systems(vec![draft(
        "zznodb",
        "System without database",
        None,
        vec!["nod".into()],
    )]);
    h.wait_publication(WAIT);
    assert_eq!(h.dialog(), DialogKind::Added);
    // The filter of the DAT update and of the cover search: no libretro name, no job.
    let system = emu_ui::library_db::system("zznodb").expect("published");
    assert!(system.libretro_name().is_none());

    // A game under its folder, scanned, shown; then the searches the screen can start.
    let dir = root.dirs.root.join("roms").join("zznodb");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("No Db Game.nod"), [7_u8; 64]).unwrap();
    scan(&root);
    let mut h = LibraryHarness::open(root.dirs.clone(), Config::default());
    h.set_dat_source(&format!("{}/", server.uri()));
    assert_eq!(h.games(), ["No Db Game"]);
    h.search_database("zznodb");
    h.wait_dat_work(WAIT);
    for _ in 0..20 {
        h.frame();
        std::thread::sleep(Duration::from_millis(10));
    }

    let requests = rt.block_on(server.received_requests()).unwrap();
    assert!(requests.is_empty(), "requests made: {requests:?}");
    assert!(
        all_files(&root.dirs.database).is_empty(),
        "no DAT file: {:?}",
        all_files(&root.dirs.database)
    );
    assert!(
        all_files(&root.dirs.covers).is_empty(),
        "no cover: {:?}",
        all_files(&root.dirs.covers)
    );
    let (db, _) = Library::open(&root.dirs.library_db).unwrap();
    assert!(
        db.dat_info_for("zznodb", DatSource::NoIntro)
            .unwrap()
            .is_none()
    );
}

const INFO: &str = "display_name = \"Zz - Fictional (Fic Core)\"\n\
    corename = \"Fic Core\"\nsupported_extensions = \"fic\"\nlicense = \"MIT\"\n\
    database = \"Zz - Fictional\"\nsystemname = \"Fictional machine\"\nhw_render = \"false\"\n";

/// The complete addition: index, `info.zip`, zipped core, `addability`, definition, publication, DAT search and
/// import, a game scanned and identified by the DAT. The core is not loaded here (`custom_real.rs` does that).
#[test]
fn the_complete_addition_goes_from_the_core_index_to_a_scanned_and_identified_game() {
    let _guard = lock();
    let root = root();
    let platform = Platform::current().expect("a supported platform");
    let rt = mock_runtime();
    let server = rt.block_on(MockServer::start());
    let source = BuildbotSource::custom(&server.uri()).unwrap();
    let id = CoreId::new("zzfic").unwrap();
    let core_bytes = b"not a real library: never loaded by this test".to_vec();

    let mount = |url_path: String, body: ResponseTemplate| {
        rt.block_on(
            Mock::given(method("GET"))
                .and(path(url_path))
                .respond_with(body)
                .mount(&server),
        );
    };
    mount(
        url_path(&server, &source.index_url(platform)),
        ResponseTemplate::new(200).set_body_string(format!(
            "2026-11-20 {:08x} {}.zip\n",
            crc32(&core_bytes),
            platform.core_file_name(&id)
        )),
    );
    mount(
        url_path(&server, &source.info_bundle_url()),
        ResponseTemplate::new(200)
            .set_body_bytes(zip_of(&[("zzfic_libretro.info", INFO.as_bytes())])),
    );
    mount(
        url_path(&server, &source.core_url(platform, &id)),
        ResponseTemplate::new(200).set_body_bytes(zip_of(&[(
            platform.core_file_name(&id).as_str(),
            core_bytes.as_slice(),
        )])),
    );

    // The game and its DAT: one fictional game, found by its CRC.
    let rom: Vec<u8> = (0..=255_u8).collect();
    let dat = format!(
        "clrmamepro (\n\tname \"Zz - Fictional\"\n\tversion 20261127\n)\n\ngame (\n\tname \"Fictional Hero (World)\"\n\
         \tdescription \"Fictional Hero (World)\"\n\trom ( name \"Fictional Hero (World).fic\" size {} crc {:08x} )\n)\n",
        rom.len(),
        crc32(&rom)
    );
    mount(
        "/metadat/no-intro/Zz%20-%20Fictional.dat".to_owned(),
        ResponseTemplate::new(200).set_body_bytes(dat.into_bytes()),
    );

    // 1. Index, info bundle, core.
    let http = HttpClient::new(HttpConfig {
        allow_loopback_http: true,
        timeout: Duration::from_secs(20),
        ..HttpConfig::default()
    })
    .unwrap();
    let manager = CoreManager::open(
        CoreDirs::under(&root.dirs.root, platform),
        platform,
        source,
        http,
    )
    .unwrap();
    manager
        .refresh(&mut |_| {}, &CancelToken::new())
        .expect("index and info.zip");
    manager
        .install(&id, &mut |_| {}, &CancelToken::new())
        .expect("zipped core");
    assert!(manager.verified_path(&id).is_ok(), "core installed");

    // 2. What the `.info` allows.
    let info = manager.info(&id).expect("info of the core");
    assert!(matches!(
        info.render,
        RenderNeed::Software | RenderNeed::Unknown
    ));
    assert!(matches!(emu_ui::addability(&info), Addability::Ok { .. }));
    let extensions = cartridge_extensions(&info.extensions);
    assert_eq!(extensions, ["fic"]);

    // 3. Definition, publication, notice.
    let mut h = LibraryHarness::open(root.dirs.clone(), Config::default());
    h.add_systems(vec![draft(
        "zzfic",
        &info.display_name,
        info.databases.first().map(String::as_str),
        extensions,
    )]);
    h.wait_publication(WAIT);
    assert_eq!(h.dialog(), DialogKind::Added);
    assert!(root.dirs.custom_systems.join("zzfic.toml").is_file());

    // 4. The DAT, then a game of that DAT under the folder of the system.
    h.set_dat_source(&format!("{}/", server.uri()));
    h.search_database("zzfic");
    h.wait_dat_work(WAIT);
    let dir = root.dirs.root.join("roms").join("zzfic");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hero.fic"), &rom).unwrap();
    scan(&root);
    let h = LibraryHarness::open(root.dirs.clone(), Config::default());
    assert_eq!(h.games(), ["Fictional Hero (World)"]);
    let (db, _) = Library::open(&root.dirs.library_db).unwrap();
    assert!(
        db.dat_info_for("zzfic", DatSource::NoIntro)
            .unwrap()
            .is_some()
    );

    // Only the requests of the addition, none of the start-up.
    let requests = rt.block_on(server.received_requests()).unwrap();
    let paths: Vec<String> = requests.iter().map(|r| r.url.path().to_owned()).collect();
    assert!(
        paths.iter().any(|p| p.ends_with("Zz%20-%20Fictional.dat")),
        "{paths:?}"
    );
}
