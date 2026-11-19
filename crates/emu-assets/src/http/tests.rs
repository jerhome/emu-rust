//! Local servers only: wiremock for well-formed HTTP, a raw TCP server for misbehaving ones.
//! The blocking client must not run inside the tokio runtime, so each test owns a runtime for wiremock only.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use wiremock::matchers::{body_bytes, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

struct Server {
    // Dropped before the runtime it lives on.
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

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server.uri())
    }

    fn requests(&self) -> Vec<wiremock::Request> {
        self.rt
            .block_on(self.server.received_requests())
            .unwrap_or_default()
    }
}

fn test_config() -> HttpConfig {
    HttpConfig {
        allow_loopback_http: true,
        timeout: Duration::from_secs(10),
        ..HttpConfig::default()
    }
}

fn client() -> HttpClient {
    HttpClient::new(test_config()).unwrap()
}

fn get(url: &str) -> Request<'_> {
    Request {
        method: Method::Get,
        url,
        headers: Vec::new(),
        body: None,
        max_bytes: 1024 * 1024,
    }
}

fn fetch(client: &HttpClient, url: &str, max: u64) -> Result<Vec<u8>, AssetError> {
    client.get_with_progress(url, max, &mut |_| {}, &CancelToken::new())
}

/// One connection: the request head is consumed, then `respond` writes whatever it wants.
fn raw_server(respond: impl FnOnce(&mut TcpStream) + Send + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
            line.clear();
        }
        respond(&mut stream);
    });
    format!("http://{addr}/file")
}

#[test]
fn only_https_and_test_loopback_are_accepted() {
    let strict = HttpClient::new(HttpConfig::default()).unwrap();
    for url in [
        "http://127.0.0.1:1/x",
        "http://example.com/x",
        "ftp://example.com/x",
        "file:///etc/passwd",
        "https://user:pass@example.com/x",
        "not a url",
        "",
    ] {
        assert!(
            matches!(fetch(&strict, url, 10), Err(AssetError::UrlRefused)),
            "{url}"
        );
    }
    let loopback = client();
    for url in [
        "http://localhost:1/x",
        "http://10.0.0.1/x",
        "http://[::1]:1/x",
    ] {
        assert!(
            matches!(fetch(&loopback, url, 10), Err(AssetError::UrlRefused)),
            "{url}"
        );
    }
}

#[test]
fn default_config_is_generic_and_strict() {
    let config = HttpConfig::default();
    assert_eq!(
        config.user_agent,
        format!("emu-rust/{}", env!("CARGO_PKG_VERSION"))
    );
    assert!(config.same_host_redirects);
    assert!(!config.allow_loopback_http);
}

#[test]
fn send_returns_any_status_with_headers() {
    let server = Server::start();
    server.mount(
        Mock::given(method("POST"))
            .and(path("/api"))
            .and(header("client-id", "abc"))
            .and(header(
                "user-agent",
                format!("emu-rust/{}", env!("CARGO_PKG_VERSION")).as_str(),
            ))
            .and(body_bytes(b"query".to_vec()))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "7")
                    .insert_header("etag", "\"v1\"")
                    .set_body_raw(b"{}".to_vec(), "application/json"),
            ),
    );
    let url = server.url("/api");
    let response = client()
        .send(&Request {
            method: Method::Post,
            url: &url,
            headers: vec![("client-id", "abc".into())],
            body: Some(b"query".to_vec()),
            max_bytes: 100,
        })
        .unwrap();
    assert_eq!(response.status, 429);
    assert_eq!(response.retry_after, Some(Duration::from_secs(7)));
    assert_eq!(response.etag.as_deref(), Some("\"v1\""));
    assert_eq!(response.content_type.as_deref(), Some("application/json"));
    assert_eq!(response.body, b"{}");
}

#[test]
fn get_with_progress_reports_and_returns_the_body() {
    let server = Server::start();
    let body = vec![0xABu8; 300_000];
    server.mount(
        Mock::given(path("/core.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone())),
    );
    let mut last = Progress::default();
    let mut calls = 0;
    let got = client()
        .get_with_progress(
            &server.url("/core.zip"),
            1 << 20,
            &mut |p| {
                calls += 1;
                last = p;
            },
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(got, body);
    assert!(calls >= 2);
    assert_eq!(
        last,
        Progress {
            done: 300_000,
            total: Some(300_000)
        }
    );
}

#[test]
fn non_200_is_an_http_error() {
    let server = Server::start();
    server.mount(Mock::given(path("/gone")).respond_with(ResponseTemplate::new(404)));
    server.mount(Mock::given(path("/fail")).respond_with(ResponseTemplate::new(503)));
    server.mount(Mock::given(path("/same")).respond_with(ResponseTemplate::new(304)));
    let client = client();
    let err = fetch(&client, &server.url("/gone"), 10).unwrap_err();
    assert!(matches!(err, AssetError::Http { status: 404 }));
    assert!(!err.is_transient());
    let err = fetch(&client, &server.url("/fail"), 10).unwrap_err();
    assert!(matches!(err, AssetError::Http { status: 503 }));
    assert!(err.is_transient());
    assert!(matches!(
        fetch(&client, &server.url("/same"), 10),
        Err(AssetError::Http { status: 304 })
    ));
}

#[test]
fn announced_length_over_the_limit_is_refused_early() {
    let server = Server::start();
    server.mount(
        Mock::given(path("/big"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 5000])),
    );
    let client = client();
    assert!(matches!(
        fetch(&client, &server.url("/big"), 4999),
        Err(AssetError::TooLarge { limit: 4999 })
    ));
    assert!(matches!(
        client.send(&Request {
            max_bytes: 10,
            ..get(&server.url("/big"))
        }),
        Err(AssetError::TooLarge { limit: 10 })
    ));
    assert_eq!(
        fetch(&client, &server.url("/big"), 5000).unwrap().len(),
        5000
    );
}

#[test]
fn chunked_body_without_length_is_bounded() {
    let url = raw_server(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
        for _ in 0..20 {
            if s.write_all(b"400\r\n").is_err()
                || s.write_all(&[b'z'; 0x400]).is_err()
                || s.write_all(b"\r\n").is_err()
            {
                return;
            }
        }
        let _ = s.write_all(b"0\r\n\r\n");
    });
    assert!(matches!(
        fetch(&client(), &url, 4096),
        Err(AssetError::TooLarge { limit: 4096 })
    ));
}

#[test]
fn body_shorter_than_announced_is_a_network_error() {
    let url = raw_server(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\nonly a few bytes");
        let _ = s.shutdown(std::net::Shutdown::Both);
    });
    let err = fetch(&client(), &url, 10_000).unwrap_err();
    assert!(matches!(err, AssetError::Network(_)), "{err:?}");
    assert!(err.is_transient());
}

#[test]
fn slow_headers_hit_the_timeout() {
    let server = Server::start();
    server.mount(
        Mock::given(path("/slow"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5))),
    );
    let client = HttpClient::new(HttpConfig {
        timeout: Duration::from_millis(300),
        ..test_config()
    })
    .unwrap();
    let start = Instant::now();
    let err = fetch(&client, &server.url("/slow"), 10).unwrap_err();
    assert!(
        matches!(&err, AssetError::Network(m) if m == "délai dépassé"),
        "{err:?}"
    );
    assert!(start.elapsed() < Duration::from_secs(3));
}

/// A server sending one byte at a time never exceeds the per-read timeout but must hit the total one.
#[test]
fn trickling_body_hits_the_total_timeout() {
    let url = raw_server(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n");
        for _ in 0..1000 {
            if s.write_all(b"a").and_then(|()| s.flush()).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
    });
    let client = HttpClient::new(HttpConfig {
        timeout: Duration::from_millis(600),
        ..test_config()
    })
    .unwrap();
    let start = Instant::now();
    let err = fetch(&client, &url, 10_000).unwrap_err();
    assert!(
        matches!(&err, AssetError::Network(m) if m == "délai dépassé"),
        "{err:?}"
    );
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn cancellation_stops_the_body() {
    let url = raw_server(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\n");
        for _ in 0..100 {
            if s.write_all(&[b'c'; 1000]).and_then(|()| s.flush()).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
    });
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let result = client().get_with_progress(
        &url,
        1 << 20,
        &mut |p| {
            if p.done > 0 {
                token.cancel();
            }
        },
        &cancel,
    );
    assert!(matches!(result, Err(AssetError::Cancelled)));
    assert!(!AssetError::Cancelled.is_transient());

    let cancelled = CancelToken::new();
    cancelled.cancel();
    assert!(matches!(
        client().get_with_progress("https://example.invalid/", 1, &mut |_| {}, &cancelled),
        Err(AssetError::Cancelled)
    ));
}

#[test]
fn same_host_redirects_are_followed() {
    let server = Server::start();
    server.mount(
        Mock::given(path("/old"))
            .respond_with(ResponseTemplate::new(301).insert_header("location", "/new")),
    );
    server.mount(
        Mock::given(method("GET"))
            .and(path("/new"))
            .respond_with(ResponseTemplate::new(200).set_body_string("moved")),
    );
    server.mount(
        Mock::given(method("POST"))
            .and(path("/form"))
            .respond_with(ResponseTemplate::new(303).insert_header("location", "/new")),
    );
    let client = client();
    assert_eq!(fetch(&client, &server.url("/old"), 100).unwrap(), b"moved");
    let response = client
        .send(&Request {
            method: Method::Post,
            body: Some(b"x".to_vec()),
            ..get(&server.url("/form"))
        })
        .unwrap();
    assert_eq!(response.body, b"moved");
}

#[test]
fn redirect_loops_are_bounded() {
    let server = Server::start();
    server.mount(
        Mock::given(path("/loop"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/loop")),
    );
    let err = fetch(&client(), &server.url("/loop"), 100).unwrap_err();
    assert!(
        matches!(&err, AssetError::Network(m) if m.contains("redirections")),
        "{err:?}"
    );
    assert_eq!(server.requests().len(), MAX_REDIRECTS + 1);
}

#[test]
fn redirect_to_another_host_is_refused_by_default() {
    let target = Server::start();
    target.mount(Mock::given(path("/x")).respond_with(ResponseTemplate::new(200)));
    let server = Server::start();
    server.mount(
        Mock::given(path("/away"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", target.url("/x"))),
    );
    server.mount(Mock::given(path("/insecure")).respond_with(
        ResponseTemplate::new(302).insert_header("location", "http://example.com/x"),
    ));
    let client = client();
    assert!(matches!(
        fetch(&client, &server.url("/away"), 100),
        Err(AssetError::UrlRefused)
    ));
    assert!(target.requests().is_empty());
    assert!(matches!(
        fetch(&client, &server.url("/insecure"), 100),
        Err(AssetError::UrlRefused)
    ));
}

#[test]
fn cross_host_redirect_drops_custom_headers_when_allowed() {
    let target = Server::start();
    target.mount(
        Mock::given(path("/x")).respond_with(ResponseTemplate::new(200).set_body_string("there")),
    );
    let server = Server::start();
    server.mount(
        Mock::given(path("/away"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", target.url("/x"))),
    );
    let client = HttpClient::new(HttpConfig {
        same_host_redirects: false,
        ..test_config()
    })
    .unwrap();
    let url = server.url("/away");
    let response = client
        .send(&Request {
            headers: vec![("authorization", "Bearer secret".into())],
            ..get(&url)
        })
        .unwrap();
    assert_eq!(response.body, b"there");
    let forwarded = target.requests();
    assert_eq!(forwarded.len(), 1);
    assert!(forwarded[0].headers.get("authorization").is_none());
}

#[test]
fn download_to_file_is_atomic() {
    let server = Server::start();
    server.mount(
        Mock::given(path("/ok")).respond_with(ResponseTemplate::new(200).set_body_string("new")),
    );
    server.mount(
        Mock::given(path("/big"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1u8; 100])),
    );
    server.mount(Mock::given(path("/gone")).respond_with(ResponseTemplate::new(404)));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("file.dat");
    std::fs::write(&dest, "old").unwrap();
    let client = client();

    assert!(matches!(
        client.download_to_file(&server.url("/big"), &dest, 50),
        Err(AssetError::TooLarge { limit: 50 })
    ));
    assert!(matches!(
        client.download_to_file(&server.url("/gone"), &dest, 50),
        Err(AssetError::Http { status: 404 })
    ));
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");

    assert_eq!(
        client
            .download_to_file(&server.url("/ok"), &dest, 50)
            .unwrap(),
        3
    );
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new");
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("file.dat")]);

    let missing_dir = dir.path().join("missing/file.dat");
    assert!(matches!(
        client.download_to_file(&server.url("/ok"), &missing_dir, 50),
        Err(AssetError::Io { .. })
    ));
}

#[test]
fn network_errors_never_contain_the_url() {
    // Bound then dropped: nothing listens on this port any more.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}/api?devpassword=hunter2&ssid=me");
    let err = client()
        .send(&Request {
            headers: vec![("authorization", "Bearer token-xyz".into())],
            ..get(&url)
        })
        .unwrap_err();
    let message = format!("{err} {err:?}");
    assert!(matches!(err, AssetError::Network(_)), "{message}");
    assert!(err.is_transient());
    for secret in ["hunter2", "devpassword", "token-xyz", "/api"] {
        assert!(!message.contains(secret), "{message}");
    }
}

/// Real TLS handshake with the platform verifier and ring.
#[test]
#[ignore = "réseau réel"]
fn real_https_request() {
    let client = HttpClient::new(HttpConfig::default()).unwrap();
    let body = fetch(
        &client,
        "https://buildbot.libretro.com/nightly/linux/x86_64/latest/.index-extended",
        1024 * 1024,
    )
    .unwrap();
    assert!(!body.is_empty());
}

#[test]
fn cross_host_307_with_body_is_refused() {
    let target = Server::start();
    target.mount(Mock::given(path("/x")).respond_with(ResponseTemplate::new(200)));
    let server = Server::start();
    server.mount(
        Mock::given(path("/away"))
            .respond_with(ResponseTemplate::new(307).insert_header("location", target.url("/x"))),
    );
    let client = HttpClient::new(HttpConfig {
        same_host_redirects: false,
        ..test_config()
    })
    .unwrap();
    let url = server.url("/away");
    let err = client
        .send(&Request {
            method: Method::Post,
            body: Some(b"password=secret".to_vec()),
            ..get(&url)
        })
        .unwrap_err();
    assert!(matches!(err, AssetError::UrlRefused));
    assert!(target.requests().is_empty());
}

#[test]
fn request_debug_hides_header_values_and_query() {
    let request = Request {
        headers: vec![("authorization", "Bearer secret-token".into())],
        body: Some(b"password=hunter2".to_vec()),
        ..get("https://example.com/api?ssid=user&sspassword=pw")
    };
    let text = format!("{request:?}");
    assert!(text.contains("authorization"));
    for secret in ["secret-token", "hunter2", "sspassword", "pw"] {
        assert!(!text.contains(secret), "{secret} leaked in {text}");
    }
}
