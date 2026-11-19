//! The only HTTP client of the workspace (ADR 0014 §4, shared with the core manager of ADR 0013): reqwest blocking,
//! rustls with `ring`, platform certificates. Blocking calls: use from worker threads only, never the UI thread.
//!
//! Redirections are followed here, not by reqwest, so that every hop goes through the same URL check
//! (HTTPS only, same scheme + host + port by default) and custom headers (possibly credentials) never leave
//! the original origin.

use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Url;
use reqwest::header::{CONTENT_TYPE, ETAG, LOCATION, RETRY_AFTER};

use crate::AssetError;

/// Hops followed after the first request.
pub const MAX_REDIRECTS: usize = 3;
const CHUNK_SIZE: usize = 64 * 1024;
/// Initial buffer for in-memory bodies; Content-Length is never trusted for an allocation.
const MAX_INITIAL_CAPACITY: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpConfig {
    /// Generic, no user or machine detail (ADR 0005): `emu-rust/<version>`.
    pub user_agent: String,
    pub connect_timeout: Duration,
    /// Whole request, body included. A body read already in progress may overrun it by at most one
    /// stalled read (bounded by the remaining time when the request was sent).
    pub timeout: Duration,
    /// Tests only (wiremock): also accept `http://127.0.0.1:<port>`.
    pub allow_loopback_http: bool,
    /// Follow redirections only towards the scheme + host + port of the requested URL (at most
    /// [`MAX_REDIRECTS`]). When `false`, other HTTPS hosts are followed too, but the request's custom headers
    /// are dropped as soon as the origin changes.
    pub same_host_redirects: bool,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            user_agent: format!("emu-rust/{}", env!("CARGO_PKG_VERSION")),
            connect_timeout: Duration::from_secs(10),
            timeout: Duration::from_secs(60),
            allow_loopback_http: false,
            same_host_redirects: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub etag: Option<String>,
    pub content_type: Option<String>,
    pub retry_after: Option<Duration>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// Header values may be secrets: they are never logged (`Debug` shows header names only, and the body size).
#[derive(Clone, PartialEq, Eq)]
pub struct Request<'a> {
    pub method: Method,
    pub url: &'a str,
    pub headers: Vec<(&'static str, String)>,
    pub body: Option<Vec<u8>>,
    /// Reading stops with `AssetError::TooLarge` past this many body bytes (Content-Length is not trusted).
    pub max_bytes: u64,
}

impl fmt::Debug for Request<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The URL may carry credentials in its query string (ScreenScraper): only scheme, host and path.
        let url = Url::parse(self.url).map_or_else(
            |_| "<invalide>".to_owned(),
            |u| {
                format!(
                    "{}://{}{}",
                    u.scheme(),
                    u.host_str().unwrap_or(""),
                    u.path()
                )
            },
        );
        let headers: Vec<_> = self
            .headers
            .iter()
            .map(|(name, _)| (*name, "***"))
            .collect();
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &url)
            .field("headers", &headers)
            .field("body_len", &self.body.as_ref().map(Vec::len))
            .field("max_bytes", &self.max_bytes)
            .finish()
    }
}

/// Cheap to clone (shared connection pool).
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::blocking::Client,
    config: Arc<HttpConfig>,
}

impl HttpClient {
    pub fn new(config: HttpConfig) -> Result<Self, AssetError> {
        use rustls_platform_verifier::BuilderVerifierExt;

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(|b| b.with_platform_verifier())
            .map_err(|e| AssetError::Network(format!("TLS : {e}")))?
            .with_no_client_auth();
        let mut builder = reqwest::blocking::Client::builder()
            .tls_backend_preconfigured(tls)
            .user_agent(config.user_agent.clone())
            .connect_timeout(config.connect_timeout)
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            // Belt and braces: `check_url` already refuses anything else.
            .https_only(!config.allow_loopback_http);
        if config.allow_loopback_http {
            // Tests must reach the local mock server even when the environment sets a proxy.
            builder = builder.no_proxy();
        }
        let inner = builder.build().map_err(|e| network_error(e, None))?;
        Ok(Self {
            inner,
            config: Arc::new(config),
        })
    }

    pub fn config(&self) -> &HttpConfig {
        &self.config
    }

    /// Any status is returned as `Ok` (callers map 404, 401, 429…); transport errors are `Network` without the URL.
    pub fn send(&self, request: &Request<'_>) -> Result<Response, AssetError> {
        let deadline = self.deadline();
        let mut response = self.execute(
            request.method,
            request.url,
            &request.headers,
            request.body.as_deref(),
            deadline,
        )?;
        let status = response.status().as_u16();
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let etag = header(ETAG);
        let content_type = header(CONTENT_TYPE);
        let retry_after = header(RETRY_AFTER)
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let mut body = Vec::new();
        self.read_body(
            &mut response,
            request.max_bytes,
            deadline,
            None,
            &mut |_| {},
            &mut |chunk| {
                body.extend_from_slice(chunk);
                Ok(())
            },
        )?;
        Ok(Response {
            status,
            etag,
            content_type,
            retry_after,
            body,
        })
    }

    /// Streams a 200 body to a temporary file next to `dest`, then renames it (ADR 0005). Other statuses are
    /// `AssetError::Http`, 304 included (callers needing it use `send`). On any error `dest` is untouched and
    /// the temporary file is removed.
    pub fn download_to_file(
        &self,
        url: &str,
        dest: &Path,
        max_bytes: u64,
    ) -> Result<u64, AssetError> {
        let deadline = self.deadline();
        let mut response = self.execute(Method::Get, url, &[], None, deadline)?;
        expect_ok(&response)?;

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
            .ok_or_else(|| io_err(io::Error::from(io::ErrorKind::InvalidInput)))?;
        let mut temp = tempfile::Builder::new()
            .prefix(&format!(".{}.tmp-", file_name.to_string_lossy()))
            .tempfile_in(parent)
            .map_err(io_err)?;
        let written = self.read_body(
            &mut response,
            max_bytes,
            deadline,
            None,
            &mut |_| {},
            &mut |chunk| temp.write_all(chunk).map_err(io_err),
        )?;
        temp.as_file().sync_all().map_err(io_err)?;
        temp.persist(dest).map_err(|e| io_err(e.error))?;
        sync_dir(parent);
        Ok(written)
    }

    /// GET whose body is kept in memory, with progress and cancellation checked between chunks (core zips,
    /// index, `info.zip`). Only 200 is accepted, anything else is `AssetError::Http`.
    pub fn get_with_progress(
        &self,
        url: &str,
        max_bytes: u64,
        progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<Vec<u8>, AssetError> {
        if cancel.is_cancelled() {
            return Err(AssetError::Cancelled);
        }
        let deadline = self.deadline();
        let mut response = self.execute(Method::Get, url, &[], None, deadline)?;
        expect_ok(&response)?;
        let capacity = response
            .content_length()
            .unwrap_or(0)
            .min(max_bytes)
            .min(MAX_INITIAL_CAPACITY);
        let mut body = Vec::with_capacity(usize::try_from(capacity).unwrap_or(0));
        self.read_body(
            &mut response,
            max_bytes,
            deadline,
            Some(cancel),
            progress,
            &mut |chunk| {
                body.extend_from_slice(chunk);
                Ok(())
            },
        )?;
        Ok(body)
    }

    fn deadline(&self) -> Instant {
        Instant::now() + self.config.timeout
    }

    /// `https://<host>…` only (plus `http://127.0.0.1` in tests); no user info (it would end up in logs).
    fn check_url(&self, url: &str) -> Result<Url, AssetError> {
        let url = Url::parse(url).map_err(|_| AssetError::UrlRefused)?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(AssetError::UrlRefused);
        }
        let allowed = match url.scheme() {
            "https" => url.host_str().is_some_and(|h| !h.is_empty()),
            "http" => self.config.allow_loopback_http && url.host_str() == Some("127.0.0.1"),
            _ => false,
        };
        if allowed {
            Ok(url)
        } else {
            Err(AssetError::UrlRefused)
        }
    }

    fn execute(
        &self,
        method: Method,
        url: &str,
        headers: &[(&'static str, String)],
        body: Option<&[u8]>,
        deadline: Instant,
    ) -> Result<reqwest::blocking::Response, AssetError> {
        let origin = self.check_url(url)?;
        let mut current = origin.clone();
        let mut method = method;
        let mut body = body;
        let mut keep_headers = true;
        let mut hops = 0;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(timed_out());
            }
            let mut builder = match method {
                Method::Get => self.inner.get(current.clone()),
                Method::Post => self.inner.post(current.clone()),
            };
            if keep_headers {
                for (name, value) in headers {
                    builder = builder.header(*name, value.as_str());
                }
            }
            if let Some(body) = body {
                builder = builder.body(body.to_vec());
            }
            let response = builder
                .timeout(remaining)
                .send()
                .map_err(|e| network_error(e, Some(&current)))?;
            let status = response.status();
            if !status.is_redirection() || status == reqwest::StatusCode::NOT_MODIFIED {
                return Ok(response);
            }
            let Some(location) = response.headers().get(LOCATION) else {
                return Ok(response);
            };
            if hops == MAX_REDIRECTS {
                return Err(AssetError::Network("trop de redirections".into()));
            }
            hops += 1;
            let location = location
                .to_str()
                .map_err(|_| AssetError::InvalidResponse("redirection invalide".into()))?;
            let next = current.join(location).map_err(|_| AssetError::UrlRefused)?;
            let next = self.check_url(next.as_str())?;
            let same_origin = same_origin(&next, &origin);
            if !same_origin {
                if self.config.same_host_redirects {
                    tracing::warn!(
                        "redirection vers un autre hôte refusée ({})",
                        next.host_str().unwrap_or("?")
                    );
                    return Err(AssetError::UrlRefused);
                }
                // 307 / 308 replay the body: it may hold credentials (form fields) meant for the origin.
                if body.is_some() && matches!(status.as_u16(), 307 | 308) {
                    tracing::warn!("redirection 307/308 avec corps vers un autre hôte refusée");
                    return Err(AssetError::UrlRefused);
                }
                keep_headers = false;
            }
            if !matches!(status.as_u16(), 307 | 308) {
                method = Method::Get;
                body = None;
            }
            current = next;
        }
    }

    /// Copies the body chunk by chunk into `sink`; returns the byte count.
    fn read_body(
        &self,
        response: &mut reqwest::blocking::Response,
        max_bytes: u64,
        deadline: Instant,
        cancel: Option<&CancelToken>,
        progress: &mut dyn FnMut(Progress),
        sink: &mut dyn FnMut(&[u8]) -> Result<(), AssetError>,
    ) -> Result<u64, AssetError> {
        let total = response.content_length();
        // Early refusal only: a smaller Content-Length is never taken as a bound.
        if total.is_some_and(|t| t > max_bytes) {
            return Err(AssetError::TooLarge { limit: max_bytes });
        }
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut done = 0u64;
        progress(Progress { done, total });
        loop {
            if cancel.is_some_and(CancelToken::is_cancelled) {
                return Err(AssetError::Cancelled);
            }
            let n = match response.read(&mut buf) {
                Ok(0) => return Ok(done),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(body_error(&e)),
            };
            done += n as u64;
            if done > max_bytes {
                return Err(AssetError::TooLarge { limit: max_bytes });
            }
            sink(&buf[..n])?;
            progress(Progress { done, total });
            if Instant::now() > deadline {
                return Err(timed_out());
            }
        }
    }
}

fn expect_ok(response: &reqwest::blocking::Response) -> Result<(), AssetError> {
    match response.status().as_u16() {
        200 => Ok(()),
        status => Err(AssetError::Http { status }),
    }
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn timed_out() -> AssetError {
    AssetError::Network("délai dépassé".into())
}

/// Never includes the URL (ADR 0014 §4: query strings may carry credentials). The source chain is kept for
/// diagnostics, with the query string of `url` masked in case a lower layer echoes it.
fn network_error(error: reqwest::Error, url: Option<&Url>) -> AssetError {
    let error = error.without_url();
    if error.is_timeout() {
        return timed_out();
    }
    let mut message = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(s) = source {
        message.push_str(" : ");
        message.push_str(&s.to_string());
        source = s.source();
    }
    if let Some(query) = url.and_then(Url::query).filter(|q| !q.is_empty()) {
        message = message.replace(query, "***");
    }
    AssetError::Network(message)
}

/// Body read errors only expose their kind: the wrapped reqwest error is not guaranteed to be URL-free.
fn body_error(error: &io::Error) -> AssetError {
    let mut source = error
        .get_ref()
        .map(|e| e as &(dyn std::error::Error + 'static));
    while let Some(s) = source {
        if s.downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)
        {
            return timed_out();
        }
        source = s.source();
    }
    if error.kind() == io::ErrorKind::TimedOut {
        return timed_out();
    }
    AssetError::Network(format!("connexion interrompue ({:?})", error.kind()))
}

/// Makes the rename durable (Unix); best effort, the data itself is already synced.
pub(crate) fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Err(e) = std::fs::File::open(dir).and_then(|d| d.sync_all()) {
        tracing::debug!("fsync de {} impossible : {e}", dir.display());
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Shared by every long download and `jobs::Job` (ADR 0013): cheap to clone, checked between chunks.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Download progress; `total` from Content-Length when present (display only, never trusted as a bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
}

#[cfg(test)]
mod tests;
