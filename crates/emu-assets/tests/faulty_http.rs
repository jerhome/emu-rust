//! Misbehaving servers: a raw TCP server written here for broken HTTP, wiremock for the retry path.
//! No real network. The blocking client must not run inside tokio, so wiremock gets its own runtime.

use std::io::{Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use emu_assets::AssetError;
use emu_assets::cores::{BuildbotSource, CoreDirs, CoreError, CoreManager, Platform};
use emu_assets::http::{CancelToken, HttpClient, HttpConfig, Method, Request};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::write::SimpleFileOptions;

const LIMIT: u64 = 1024 * 1024;

/// Raw server: `handler(stream, request_head)` runs on its own thread for each connection.
struct Raw {
    url: String,
    connections: Arc<AtomicUsize>,
    heads: Arc<Mutex<Vec<String>>>,
}

impl Raw {
    fn start(handler: impl Fn(TcpStream, &str) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let connections = Arc::new(AtomicUsize::new(0));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(handler);
        let (count, seen) = (connections.clone(), heads.clone());
        // Detached: ends with the test process.
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                count.fetch_add(1, Ordering::SeqCst);
                let (handler, seen) = (handler.clone(), seen.clone());
                thread::spawn(move || {
                    let mut stream = stream;
                    let head = read_head(&mut stream);
                    seen.lock().unwrap().push(head.clone());
                    handler(stream, &head);
                });
            }
        });
        Self {
            url,
            connections,
            heads,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.url)
    }
}

fn read_head(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

fn client(timeout: Duration, same_host_redirects: bool) -> HttpClient {
    HttpClient::new(HttpConfig {
        allow_loopback_http: true,
        timeout,
        same_host_redirects,
        ..HttpConfig::default()
    })
    .unwrap()
}

fn fetch(client: &HttpClient, url: &str) -> Result<Vec<u8>, AssetError> {
    client.get_with_progress(url, LIMIT, &mut |_| {}, &CancelToken::new())
}

#[test]
fn connection_cut_before_any_response() {
    let server = Raw::start(|stream, _| drop(stream));
    let err = fetch(&client(Duration::from_secs(5), true), &server.url("/x")).unwrap_err();
    assert!(matches!(err, AssetError::Network(_)), "{err:?}");
    assert!(err.is_transient());
}

#[test]
fn connection_cut_in_the_middle_of_the_body() {
    let server = Raw::start(|mut stream, _| {
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n");
        let _ = stream.write_all(&[1u8; 1000]);
        // Dropped: peer sees EOF long before Content-Length.
    });
    let started = Instant::now();
    let err = fetch(&client(Duration::from_secs(5), true), &server.url("/x")).unwrap_err();
    assert!(matches!(err, AssetError::Network(_)), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn slow_server_hits_the_timeout() {
    let server = Raw::start(|mut stream, _| {
        thread::sleep(Duration::from_secs(4));
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    });
    let started = Instant::now();
    let err = fetch(&client(Duration::from_secs(1), true), &server.url("/x")).unwrap_err();
    assert!(matches!(err, AssetError::Network(_)), "{err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn body_longer_than_content_length_is_never_over_read() {
    let server = Raw::start(|mut stream, _| {
        let mut response =
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n".to_vec();
        response.extend_from_slice(&[b'a'; 5000]);
        let _ = stream.write_all(&response);
    });
    match fetch(&client(Duration::from_secs(5), true), &server.url("/x")) {
        Ok(body) => assert_eq!(body, [b'a'; 10]),
        Err(e) => assert!(matches!(e, AssetError::Network(_)), "{e:?}"),
    }
}

#[test]
fn unbounded_body_without_content_length_stops_at_the_limit() {
    let server = Raw::start(|mut stream, _| {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
        let chunk = [0u8; 64 * 1024];
        for _ in 0..64 {
            if stream.write_all(&chunk).is_err() {
                return;
            }
        }
    });
    let err = client(Duration::from_secs(5), true)
        .get_with_progress(
            &server.url("/x"),
            128 * 1024,
            &mut |_| {},
            &CancelToken::new(),
        )
        .unwrap_err();
    assert!(
        matches!(err, AssetError::TooLarge { limit: 131_072 }),
        "{err:?}"
    );
}

#[test]
fn redirect_to_another_host_is_refused() {
    let target = Raw::start(|mut stream, _| {
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    });
    let target_url = target.url("/stolen");
    let origin = Raw::start(move |mut stream, _| {
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(response.as_bytes());
    });
    let err = fetch(&client(Duration::from_secs(5), true), &origin.url("/x")).unwrap_err();
    assert!(matches!(err, AssetError::UrlRefused), "{err:?}");
    assert_eq!(target.connections.load(Ordering::SeqCst), 0);
}

#[test]
fn cross_host_redirect_when_allowed_drops_custom_headers() {
    let target = Raw::start(|mut stream, _| {
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    });
    let target_url = target.url("/final");
    let origin = Raw::start(move |mut stream, _| {
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(response.as_bytes());
    });
    let url = origin.url("/x");
    let request = Request {
        method: Method::Get,
        url: &url,
        headers: vec![("x-secret-token", "hunter2".into())],
        body: None,
        max_bytes: LIMIT,
    };
    let response = client(Duration::from_secs(5), false)
        .send(&request)
        .unwrap();
    assert_eq!(response.body, b"ok");
    let first = origin.heads.lock().unwrap().join("\n").to_lowercase();
    let second = target.heads.lock().unwrap().join("\n").to_lowercase();
    assert!(first.contains("x-secret-token"));
    assert!(!second.contains("x-secret-token"), "{second}");
}

#[test]
fn redirect_loop_ends_with_an_error() {
    let server = Raw::start(|mut stream, head| {
        let _ = head;
        let _ = stream.write_all(
            b"HTTP/1.1 302 Found\r\nLocation: /again\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    });
    let err = fetch(&client(Duration::from_secs(5), true), &server.url("/x")).unwrap_err();
    assert!(matches!(err, AssetError::Network(_)), "{err:?}");
    assert!(server.connections.load(Ordering::SeqCst) <= 5);
}

#[test]
fn cancellation_during_the_download_discards_everything() {
    let server = Raw::start(|mut stream, _| {
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000000\r\nConnection: close\r\n\r\n");
        let chunk = [0u8; 64 * 1024];
        for _ in 0..150 {
            if stream.write_all(&chunk).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
    });
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let mut seen_bytes = 0;
    let started = Instant::now();
    let err = client(Duration::from_secs(30), true)
        .get_with_progress(
            &server.url("/x"),
            64 * 1024 * 1024,
            &mut |p| {
                seen_bytes = p.done;
                if p.done > 0 {
                    token.cancel();
                }
            },
            &cancel,
        )
        .unwrap_err();
    assert!(matches!(err, AssetError::Cancelled), "{err:?}");
    assert!(seen_bytes < 1024 * 1024, "{seen_bytes}");
    assert!(started.elapsed() < Duration::from_secs(3));

    // Already cancelled: no connection at all.
    let before = server.connections.load(Ordering::SeqCst);
    let err = fetch_cancelled(&client(Duration::from_secs(5), true), &server.url("/y"));
    assert!(matches!(err, AssetError::Cancelled));
    assert_eq!(server.connections.load(Ordering::SeqCst), before);
}

fn fetch_cancelled(client: &HttpClient, url: &str) -> AssetError {
    let cancel = CancelToken::new();
    cancel.cancel();
    client
        .get_with_progress(url, LIMIT, &mut |_| {}, &cancel)
        .unwrap_err()
}

// Manager-level retry (3 attempts, 1 s then 2 s apart) through wiremock.

struct Mock2 {
    server: MockServer,
    rt: tokio::runtime::Runtime,
}

impl Mock2 {
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

    fn hits(&self, route: &str) -> usize {
        self.rt
            .block_on(self.server.received_requests())
            .unwrap_or_default()
            .iter()
            .filter(|r| r.url.path() == route)
            .count()
    }
}

const INDEX: &str = "/nightly/linux/x86_64/latest/.index-extended";
const INFO_ZIP: &str = "/assets/frontend/info.zip";

fn manager(server: &Mock2, dir: &std::path::Path) -> CoreManager {
    let http = HttpClient::new(HttpConfig {
        allow_loopback_http: true,
        timeout: Duration::from_secs(10),
        ..HttpConfig::default()
    })
    .unwrap();
    let source = BuildbotSource::custom(&server.server.uri()).unwrap();
    let platform = Platform::LinuxX86_64;
    CoreManager::open(CoreDirs::under(dir, platform), platform, source, http).unwrap()
}

fn info_zip() -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("gambatte_libretro.info", SimpleFileOptions::default())
        .unwrap();
    writer
        .write_all(b"display_name = \"Gambatte\"\nsupported_extensions = \"gb\"\n")
        .unwrap();
    writer.finish().unwrap().into_inner()
}

#[test]
fn server_error_then_success_is_retried() {
    let server = Mock2::start();
    server.mount(
        Mock::given(path(INDEX))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1),
    );
    server.mount(
        Mock::given(path(INDEX)).respond_with(
            ResponseTemplate::new(200)
                .set_body_string("2026-11-16 e09352e9 gambatte_libretro.so.zip\n"),
        ),
    );
    server.mount(
        Mock::given(method("GET"))
            .and(path(INFO_ZIP))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(info_zip())),
    );
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(&server, dir.path());
    let started = Instant::now();
    let catalog = manager.refresh(&mut |_| {}, &CancelToken::new()).unwrap();
    assert_eq!(catalog.cores.len(), 1);
    assert_eq!(server.hits(INDEX), 2);
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "waited for the retry delay"
    );
}

#[test]
fn client_error_is_not_retried_and_leaves_no_state() {
    let server = Mock2::start();
    server.mount(Mock::given(path(INDEX)).respond_with(ResponseTemplate::new(404)));
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(&server, dir.path());
    let err = manager
        .refresh(&mut |_| {}, &CancelToken::new())
        .unwrap_err();
    assert!(
        matches!(err, CoreError::Asset(AssetError::Http { status: 404 })),
        "{err:?}"
    );
    assert_eq!(server.hits(INDEX), 1);
    assert!(manager.catalog().is_none());
}

#[test]
fn cancellation_interrupts_the_retry_wait() {
    let server = Mock2::start();
    server.mount(Mock::given(path(INDEX)).respond_with(ResponseTemplate::new(500)));
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(&server, dir.path());
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        token.cancel();
    });
    let started = Instant::now();
    let err = manager.refresh(&mut |_| {}, &cancel).unwrap_err();
    canceller.join().unwrap();
    assert!(
        matches!(err, CoreError::Asset(AssetError::Cancelled)),
        "{err:?}"
    );
    assert!(
        started.elapsed() < Duration::from_millis(1800),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(server.hits(INDEX), 1);
}
