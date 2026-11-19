//! Cover sources and DAT download against wiremock (no real network). Every log line of the process is captured
//! at TRACE level and checked for credentials.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use emu_assets::http::{HttpClient, HttpConfig};
use emu_assets::{
    AssetError, CoverChain, CoverQuery, CoverSource, CoverSourceKind, DatDownload, IgdbCredentials,
    IgdbSource, ImageFormat, LibretroThumbnails, ScreenScraperCredentials, ScreenScraperSource,
    Secret, dat_url, download_dat,
};
use wiremock::matchers::{body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DEV_PASSWORD: &str = "devpw-S3CRET-4f1a";
const USER_PASSWORD: &str = "userpw-S3CRET-9b2c";
const IGDB_SECRET: &str = "igdb-S3CRET-77de";
const FIXTURE_TOKEN: &str = "fixture-not-a-real-token";

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/data/covers/");

fn fixture(rel: &str) -> Vec<u8> {
    std::fs::read(format!("{DATA}{rel}")).unwrap()
}

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn logs() -> &'static Arc<Mutex<Vec<u8>>> {
    static LOGS: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    LOGS.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = Capture(buffer.clone());
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber).unwrap();
        buffer
    })
}

fn assert_no_secret(text: &str) {
    for secret in [DEV_PASSWORD, USER_PASSWORD, IGDB_SECRET, FIXTURE_TOKEN] {
        assert!(!text.contains(secret), "secret leaked: {text}");
    }
    let logged = String::from_utf8_lossy(&logs().lock().unwrap()).into_owned();
    for secret in [DEV_PASSWORD, USER_PASSWORD, IGDB_SECRET, FIXTURE_TOKEN] {
        assert!(!logged.contains(secret), "secret in logs: {logged}");
    }
}

/// The blocking client must not run inside tokio: wiremock gets its own runtime.
struct Server {
    server: MockServer,
    rt: tokio::runtime::Runtime,
}

impl Server {
    fn start() -> Self {
        logs();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let server = rt.block_on(MockServer::start());
        Self { server, rt }
    }

    fn uri(&self) -> String {
        self.server.uri()
    }

    fn mount(&self, mock: Mock) {
        self.rt.block_on(mock.mount(&self.server));
    }

    fn hits(&self, route: &str) -> usize {
        self.rt
            .block_on(self.server.received_requests())
            .unwrap_or_default()
            .iter()
            .filter(|r| r.url.path() == route)
            .count()
    }

    /// Fixture JSON with `{BASE}` replaced by the server URI.
    fn json(&self, rel: &str) -> String {
        String::from_utf8(fixture(rel))
            .unwrap()
            .replace("{BASE}", &self.uri())
    }
}

fn client() -> HttpClient {
    HttpClient::new(HttpConfig {
        allow_loopback_http: true,
        timeout: Duration::from_secs(10),
        ..HttpConfig::default()
    })
    .unwrap()
}

fn png() -> Vec<u8> {
    fixture("images/valid.png")
}

fn query(title: &str) -> CoverQuery {
    CoverQuery {
        title: title.into(),
        identified: true,
        file_name: format!("{title}.sfc"),
        size: 524_288,
        crc32: Some(0xDEAD_BEEF),
        sha1: Some([0x11; 20]),
        database: "Nintendo - Super Nintendo Entertainment System".into(),
        screenscraper_system: Some(4),
        igdb_platform: Some(19),
    }
}

// ----------------------------------------------------------------------------------------------------------------
// libretro-thumbnails

/// Paths as wiremock sees them (percent-encoded).
const SNES: &str = "/Nintendo%20-%20Super%20Nintendo%20Entertainment%20System/Named_Boxarts";

#[test]
fn libretro_found_and_absent() {
    let s = Server::start();
    s.mount(
        Mock::given(method("GET"))
            .and(path(
                format!("{SNES}/Fictional_%20Quest_%20%28USA%29.png").as_str(),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png())),
    );
    let mut source = LibretroThumbnails::new(client(), &s.uri());
    let found = source
        .fetch(&query("Fictional: Quest? (USA)"))
        .unwrap()
        .unwrap();
    assert_eq!(found.source, CoverSourceKind::LibretroThumbnails);
    assert_eq!(found.format, ImageFormat::Png);
    assert_eq!(found.bytes, png());
    // Unknown title: wiremock answers 404.
    assert_eq!(source.fetch(&query("Nothing")).unwrap(), None);
}

#[test]
fn libretro_invalid_image_and_size_limit() {
    let s = Server::start();
    s.mount(
        Mock::given(path(format!("{SNES}/Bad.png").as_str())).respond_with(
            ResponseTemplate::new(200).set_body_bytes(fixture("images/huge-header.png")),
        ),
    );
    s.mount(
        Mock::given(path(format!("{SNES}/Big.png").as_str())).respond_with(
            ResponseTemplate::new(200).set_body_bytes(vec![0u8; 8 * 1024 * 1024 + 1]),
        ),
    );
    let mut source = LibretroThumbnails::new(client(), &s.uri());
    assert!(matches!(
        source.fetch(&query("Bad")),
        Err(AssetError::InvalidImage(_))
    ));
    assert!(matches!(
        source.fetch(&query("Big")),
        Err(AssetError::TooLarge { .. })
    ));
}

#[test]
fn https_only_outside_loopback_tests() {
    let strict = HttpClient::new(HttpConfig::default()).unwrap();
    let mut source = LibretroThumbnails::new(strict, "http://example.invalid/");
    assert!(matches!(
        source.fetch(&query("x")),
        Err(AssetError::UrlRefused)
    ));
}

// ----------------------------------------------------------------------------------------------------------------
// ScreenScraper

fn ss_credentials() -> ScreenScraperCredentials {
    ScreenScraperCredentials {
        dev_id: "devid".into(),
        dev_password: Secret::new(DEV_PASSWORD),
        user: "someone".into(),
        password: Secret::new(USER_PASSWORD),
    }
}

fn ss(s: &Server) -> ScreenScraperSource {
    ScreenScraperSource::new(client(), ss_credentials(), &format!("{}/api2/", s.uri()))
}

#[test]
fn screenscraper_picks_world_box() {
    let s = Server::start();
    s.mount(
        Mock::given(method("GET"))
            .and(path("/api2/jeuInfos.php"))
            .and(query_param("devpassword", DEV_PASSWORD))
            .and(query_param("sspassword", USER_PASSWORD))
            .and(query_param("softname", "emu-rust"))
            .and(query_param("systemeid", "4"))
            .and(query_param("crc", "DEADBEEF"))
            .and(query_param(
                "sha1",
                "1111111111111111111111111111111111111111",
            ))
            .and(query_param("romtaille", "524288"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(s.json("screenscraper/jeuInfos.json")),
            ),
    );
    s.mount(
        Mock::given(path("/ss/media/box-2D-wor.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png())),
    );
    let found = ss(&s).fetch(&query("Fictional Quest")).unwrap().unwrap();
    assert_eq!(found.source, CoverSourceKind::ScreenScraper);
    assert_eq!(s.hits("/ss/media/box-2D-wor.png"), 1);
    assert_no_secret("");
}

#[test]
fn screenscraper_region_fallback_and_no_box() {
    let s = Server::start();
    s.mount(
        Mock::given(path("/api2/jeuInfos.php"))
            .and(query_param("romnom", "UsJp.sfc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(s.json("screenscraper/jeuInfos-regions-us-jp.json")),
            ),
    );
    s.mount(
        Mock::given(path("/api2/jeuInfos.php"))
            .and(query_param("romnom", "NoBox.sfc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(s.json("screenscraper/jeuInfos-no-box.json")),
            ),
    );
    s.mount(
        Mock::given(path("/ss/media/box-2D-us.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png())),
    );
    let mut source = ss(&s);
    assert!(source.fetch(&query("UsJp")).unwrap().is_some());
    assert_eq!(s.hits("/ss/media/box-2D-us.png"), 1);
    assert_eq!(s.hits("/ss/media/box-2D-jp.png"), 0);
    assert_eq!(source.fetch(&query("NoBox")).unwrap(), None);
    // Not found by the API.
    assert_eq!(source.fetch(&query("Unknown")).unwrap(), None);
}

#[test]
fn screenscraper_media_on_foreign_host_is_ignored() {
    let s = Server::start();
    let json = s
        .json("screenscraper/jeuInfos.json")
        .replace(&s.uri(), "https://evil.example");
    s.mount(
        Mock::given(path("/api2/jeuInfos.php"))
            .respond_with(ResponseTemplate::new(200).set_body_string(json)),
    );
    assert_eq!(ss(&s).fetch(&query("x")).unwrap(), None);
}

#[test]
fn screenscraper_errors_without_secrets() {
    for (status, quota) in [
        (401, false),
        (403, false),
        (429, true),
        (430, true),
        (431, true),
    ] {
        let s = Server::start();
        s.mount(Mock::given(path("/api2/jeuInfos.php")).respond_with(
            ResponseTemplate::new(status).set_body_string(format!("echo {DEV_PASSWORD}")),
        ));
        let err = ss(&s).fetch(&query("x")).unwrap_err();
        if quota {
            assert!(matches!(err, AssetError::Quota { .. }), "{status}");
        } else {
            assert!(matches!(err, AssetError::Unauthorized), "{status}");
        }
        assert_no_secret(&format!("{err} {err:?}"));
    }
    // Broken JSON, transport error.
    let s = Server::start();
    s.mount(Mock::given(path("/api2/jeuInfos.php")).respond_with(
        ResponseTemplate::new(200).set_body_string(format!("{{\"x\": \"{USER_PASSWORD}")),
    ));
    let err = ss(&s).fetch(&query("x")).unwrap_err();
    assert!(matches!(err, AssetError::InvalidResponse(_)));
    assert_no_secret(&format!("{err} {err:?}"));
    let dead = ScreenScraperSource::new(client(), ss_credentials(), "http://127.0.0.1:1/api2/");
    let err = CoverChain::new(vec![Box::new(dead)])
        .fetch(&query("x"))
        .unwrap_err();
    assert!(matches!(err, AssetError::Network(_)));
    assert_no_secret(&format!("{err} {err:?}"));
}

#[test]
fn screenscraper_incomplete_credentials_are_refused_locally() {
    let s = Server::start();
    let mut creds = ss_credentials();
    creds.dev_password = Secret::default();
    let mut source = ScreenScraperSource::new(client(), creds, &s.uri());
    assert!(matches!(
        source.fetch(&query("x")),
        Err(AssetError::Unauthorized)
    ));
    assert_eq!(s.hits("/jeuInfos.php"), 0);
}

// ----------------------------------------------------------------------------------------------------------------
// IGDB

fn igdb(s: &Server) -> IgdbSource {
    IgdbSource::new(
        client(),
        IgdbCredentials {
            client_id: "cid".into(),
            client_secret: Secret::new(IGDB_SECRET),
        },
        &format!("{}/oauth2/token", s.uri()),
        &format!("{}/v4/", s.uri()),
        &format!("{}/img/", s.uri()),
    )
}

fn mount_token(s: &Server) {
    s.mount(
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string_contains(IGDB_SECRET))
            .and(body_string_contains("grant_type=client_credentials"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("igdb/token.json"))),
    );
}

#[test]
fn igdb_token_search_and_image() {
    let s = Server::start();
    mount_token(&s);
    s.mount(
        Mock::given(method("POST"))
            .and(path("/v4/games"))
            .and(header("Client-ID", "cid"))
            .and(header(
                "Authorization",
                format!("Bearer {FIXTURE_TOKEN}").as_str(),
            ))
            .and(body_string_contains("search \"Fictional Quest II\""))
            .and(body_string_contains("where platforms = (19)"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("igdb/games.json"))),
    );
    s.mount(
        Mock::given(path("/img/fxcover02.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png())),
    );
    let mut source = igdb(&s);
    let started = Instant::now();
    // Exact name preferred over the first result.
    let found = source
        .fetch(&query("Fictional Quest II (USA)"))
        .unwrap()
        .unwrap();
    assert_eq!(found.source, CoverSourceKind::Igdb);
    assert_eq!(
        found.format,
        ImageFormat::Png,
        "format from the bytes, not the URL"
    );
    assert!(
        source
            .fetch(&query("Fictional Quest II"))
            .unwrap()
            .is_some()
    );
    assert_eq!(s.hits("/oauth2/token"), 1, "token kept in memory");
    // 5 requests at ≤ 4/s.
    assert!(started.elapsed() >= Duration::from_millis(1000));
    assert_no_secret("");
}

#[test]
fn igdb_401_renews_once_then_stops() {
    let s = Server::start();
    mount_token(&s);
    s.mount(Mock::given(path("/v4/games")).respond_with(ResponseTemplate::new(401)));
    let err = igdb(&s).fetch(&query("x")).unwrap_err();
    assert!(matches!(err, AssetError::Unauthorized));
    assert_eq!(s.hits("/oauth2/token"), 2);
    assert_eq!(s.hits("/v4/games"), 2);
    assert_no_secret(&format!("{err:?}"));

    let s = Server::start();
    s.mount(Mock::given(path("/oauth2/token")).respond_with(ResponseTemplate::new(400)));
    assert!(matches!(
        igdb(&s).fetch(&query("x")),
        Err(AssetError::Unauthorized)
    ));
}

#[test]
fn igdb_429_retry_after_suspends_the_source() {
    let s = Server::start();
    mount_token(&s);
    s.mount(
        Mock::given(path("/v4/games"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "7"))
            .up_to_n_times(1),
    );
    s.mount(
        Mock::given(path("/v4/games"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]")),
    );
    let mut source = igdb(&s);
    match source.fetch(&query("x")) {
        Err(AssetError::Quota { retry_after }) => {
            assert_eq!(retry_after, Some(Duration::from_secs(7)))
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(source.fetch(&query("x")).unwrap(), None);

    let mut chain = CoverChain::new(vec![Box::new(igdb(&s))]);
    assert_eq!(chain.fetch(&query("x")).unwrap(), None);
}

// ----------------------------------------------------------------------------------------------------------------
// Chain over real sources

#[test]
fn chain_libretro_then_screenscraper() {
    let s = Server::start();
    s.mount(Mock::given(path("/api2/jeuInfos.php")).respond_with(
        ResponseTemplate::new(200).set_body_string(s.json("screenscraper/jeuInfos.json")),
    ));
    s.mount(
        Mock::given(path("/ss/media/box-2D-wor.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png())),
    );
    let mut chain = CoverChain::new(vec![
        Box::new(LibretroThumbnails::new(
            client(),
            &format!("{}/thumbs/", s.uri()),
        )),
        Box::new(ss(&s)),
    ]);
    let found = chain.fetch(&query("Fictional Quest")).unwrap().unwrap();
    assert_eq!(found.source, CoverSourceKind::ScreenScraper);
    assert_no_secret("");
}

// ----------------------------------------------------------------------------------------------------------------
// DAT

const DAT_PATH: &str = "/no-intro/Nintendo%20-%20Game%20Boy.dat";

#[test]
fn dat_download_etag_and_not_modified() {
    let s = Server::start();
    let sample = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/data/dat/sample.dat"
    ))
    .unwrap();
    s.mount(
        Mock::given(path(DAT_PATH))
            .and(header("If-None-Match", "\"v1\""))
            .respond_with(ResponseTemplate::new(304)),
    );
    s.mount(
        Mock::given(path(DAT_PATH)).respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "\"v1\"")
                .set_body_bytes(sample.clone()),
        ),
    );
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("database/no-intro/Nintendo - Game Boy.dat");
    let url = dat_url(&format!("{}/no-intro/", s.uri()), "Nintendo - Game Boy");
    let first = download_dat(&client(), &url, &dest, None).unwrap();
    assert_eq!(
        first,
        DatDownload::Updated {
            etag: Some("\"v1\"".into()),
            bytes: sample.len() as u64
        }
    );
    assert_eq!(std::fs::read(&dest).unwrap(), sample);

    std::fs::write(&dest, b"local").unwrap();
    assert_eq!(
        download_dat(&client(), &url, &dest, Some("\"v1\"")).unwrap(),
        DatDownload::NotModified
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"local");
    assert_no_temp(dest.parent().unwrap());
}

#[test]
fn dat_errors_leave_dest_untouched() {
    let s = Server::start();
    s.mount(
        Mock::given(path("/big.dat")).respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![b'x'; emu_assets::MAX_DAT_BYTES as usize + 1]),
        ),
    );
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("x.dat");
    std::fs::write(&dest, b"old").unwrap();
    let err = download_dat(&client(), &format!("{}/big.dat", s.uri()), &dest, None).unwrap_err();
    assert!(matches!(err, AssetError::TooLarge { .. }));
    let err = download_dat(
        &client(),
        &format!("{}/missing.dat", s.uri()),
        &dest,
        Some("e"),
    )
    .unwrap_err();
    assert!(matches!(err, AssetError::Http { status: 404 }));
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    assert_no_temp(dir.path());
    let strict = HttpClient::new(HttpConfig::default()).unwrap();
    assert!(matches!(
        download_dat(&strict, "http://example.invalid/x.dat", &dest, None),
        Err(AssetError::UrlRefused)
    ));
}

fn assert_no_temp(dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name();
        assert!(!name.to_string_lossy().contains(".tmp-"), "{name:?}");
    }
}
