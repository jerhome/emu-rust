//! Cover sources (ADR 0014 §5) and the disk cache. No threads here: the queue lives in `emu-ui`, which owns the
//! database side (`emu-library`). Every call is blocking: worker threads only.
//!
//! Credentials never reach a log line or an error: ScreenScraper puts them in query strings (its media URLs too),
//! so no URL built or received here is ever logged; `AssetError` carries no URL by construction.

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use percent_encoding::utf8_percent_encode;
use reqwest::Url;
use serde::Deserialize;

use crate::dat::{PATH_SEGMENT, write_atomic};
use crate::http::{HttpClient, Method, Request};
use crate::{AssetError, ImageFormat, MAX_COVER_BYTES, MAX_COVER_SIDE, Secret, sniff_format};

/// JSON answers (ScreenScraper `jeuInfos`, IGDB search, Twitch token).
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;
/// Longest `Retry-After` honoured; a longer one suspends the source for that long only.
const MAX_SUSPEND: Duration = Duration::from_secs(3600);
/// Consecutive transient errors after which a source is suspended for the session (ADR 0014: offline detection).
const MAX_NETWORK_ERRORS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoverSourceKind {
    LibretroThumbnails,
    ScreenScraper,
    Igdb,
}

impl CoverSourceKind {
    /// Persisted (config `covers.sources`, database `CoverState::Cached::source`).
    pub fn name(self) -> &'static str {
        match self {
            CoverSourceKind::LibretroThumbnails => "libretro",
            CoverSourceKind::ScreenScraper => "screenscraper",
            CoverSourceKind::Igdb => "igdb",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::LibretroThumbnails, Self::ScreenScraper, Self::Igdb]
            .into_iter()
            .find(|k| k.name() == name)
    }
}

/// Everything a source may use; filled by `emu-ui` from `emu_library::{Game, SystemInfo}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverQuery {
    /// DAT game name when identified, else the file stem.
    pub title: String,
    pub identified: bool,
    pub file_name: String,
    pub size: u64,
    pub crc32: Option<u32>,
    pub sha1: Option<[u8; 20]>,
    /// `SystemInfo::database` (libretro-thumbnails folder).
    pub database: String,
    pub screenscraper_system: Option<u32>,
    pub igdb_platform: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverFound {
    pub source: CoverSourceKind,
    pub format: ImageFormat,
    /// Raw downloaded bytes, already checked by `decode_cover` (≤ `MAX_COVER_BYTES`).
    pub bytes: Vec<u8>,
}

/// One cover provider. `Ok(None)` = not found; `Err(Unauthorized | Quota)` makes `CoverChain` skip the source.
pub trait CoverSource: Send {
    fn kind(&self) -> CoverSourceKind;
    fn fetch(&mut self, query: &CoverQuery) -> Result<Option<CoverFound>, AssetError>;
}

/// libretro-thumbnails file name: `&*/:`<>?\|"` replaced by `_` (libretro rule), plus `.png`.
pub fn thumbnail_file_name(title: &str) -> String {
    let mut name: String = title
        .chars()
        .map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c })
        .collect();
    name.push_str(".png");
    name
}

/// GET of an image, then validation (signature + full decode). `404` = absent.
fn fetch_image(
    client: &HttpClient,
    url: &str,
    headers: Vec<(&'static str, String)>,
    source: CoverSourceKind,
) -> Result<Option<CoverFound>, AssetError> {
    let response = client.send(&Request {
        method: Method::Get,
        url,
        headers,
        body: None,
        max_bytes: MAX_COVER_BYTES,
    })?;
    match response.status {
        200 => {}
        404 | 410 => return Ok(None),
        status => return Err(status_error(status, response.retry_after)),
    }
    let format = sniff_format(&response.body)
        .ok_or_else(|| AssetError::InvalidImage("format non pris en charge".into()))?;
    crate::decode_cover(&response.body, MAX_COVER_SIDE)?;
    Ok(Some(CoverFound {
        source,
        format,
        bytes: response.body,
    }))
}

/// Common mapping of the statuses a source did not handle itself.
fn status_error(status: u16, retry_after: Option<Duration>) -> AssetError {
    match status {
        401 | 403 => AssetError::Unauthorized,
        429 => AssetError::Quota { retry_after },
        status => AssetError::Http { status },
    }
}

fn with_slash(base: &str) -> String {
    if base.ends_with('/') {
        base.to_owned()
    } else {
        format!("{base}/")
    }
}

// ---------------------------------------------------------------------------------------------------------------
// libretro-thumbnails

/// `https://thumbnails.libretro.com/<system>/Named_Boxarts/<name>.png`, no account.
pub struct LibretroThumbnails {
    client: HttpClient,
    base_url: String,
}

impl LibretroThumbnails {
    pub const BASE_URL: &'static str = "https://thumbnails.libretro.com/";

    pub fn new(client: HttpClient, base_url: &str) -> Self {
        Self {
            client,
            base_url: with_slash(base_url),
        }
    }

    pub fn url(&self, query: &CoverQuery) -> String {
        format!(
            "{}{}/Named_Boxarts/{}",
            self.base_url,
            utf8_percent_encode(&query.database, PATH_SEGMENT),
            utf8_percent_encode(&thumbnail_file_name(&query.title), PATH_SEGMENT)
        )
    }
}

impl CoverSource for LibretroThumbnails {
    fn kind(&self) -> CoverSourceKind {
        CoverSourceKind::LibretroThumbnails
    }

    fn fetch(&mut self, query: &CoverQuery) -> Result<Option<CoverFound>, AssetError> {
        if query.database.is_empty() || query.title.trim().is_empty() {
            return Ok(None);
        }
        fetch_image(&self.client, &self.url(query), Vec::new(), self.kind())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// ScreenScraper

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenScraperCredentials {
    pub dev_id: String,
    pub dev_password: Secret,
    pub user: String,
    pub password: Secret,
}

impl ScreenScraperCredentials {
    /// All four fields are required (ADR 0014: no developer credentials are published by the project).
    pub fn is_complete(&self) -> bool {
        !(self.dev_id.is_empty()
            || self.dev_password.is_empty()
            || self.user.is_empty()
            || self.password.is_empty())
    }
}

/// API v2 `jeuInfos.php` by CRC32 / SHA1 / size / name and system id; one request at a time, stops for the
/// session on quota (HTTP 429/430) or bad credentials (401/403).
pub struct ScreenScraperSource {
    client: HttpClient,
    credentials: ScreenScraperCredentials,
    base_url: String,
}

/// Media preference (ADR 0014 §2): world, Europe, USA, Japan, then the first one.
const SS_REGIONS: [&str; 4] = ["wor", "eu", "us", "jp"];

#[derive(Deserialize)]
struct SsRoot {
    #[serde(default)]
    response: Option<SsResponse>,
}

#[derive(Deserialize)]
struct SsResponse {
    #[serde(default)]
    jeu: Option<SsGame>,
}

#[derive(Deserialize)]
struct SsGame {
    #[serde(default)]
    medias: Vec<SsMedia>,
}

#[derive(Deserialize)]
struct SsMedia {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    region: Option<String>,
}

impl ScreenScraperSource {
    pub const BASE_URL: &'static str = "https://api.screenscraper.fr/api2/";

    pub fn new(client: HttpClient, credentials: ScreenScraperCredentials, base_url: &str) -> Self {
        Self {
            client,
            credentials,
            base_url: with_slash(base_url),
        }
    }

    /// Carries the credentials: never log it.
    fn info_url(&self, query: &CoverQuery, system: u32) -> Result<Url, AssetError> {
        let mut url = Url::parse(&self.base_url)
            .and_then(|b| b.join("jeuInfos.php"))
            .map_err(|_| AssetError::UrlRefused)?;
        {
            let c = &self.credentials;
            let mut q = url.query_pairs_mut();
            q.append_pair("devid", &c.dev_id)
                .append_pair("devpassword", c.dev_password.expose())
                .append_pair("softname", "emu-rust")
                .append_pair("output", "json")
                .append_pair("ssid", &c.user)
                .append_pair("sspassword", c.password.expose())
                .append_pair("systemeid", &system.to_string())
                .append_pair("romtype", "rom");
            if let Some(crc) = query.crc32 {
                q.append_pair("crc", &format!("{crc:08X}"));
            }
            if let Some(sha1) = &query.sha1 {
                q.append_pair("sha1", &hex(sha1));
            }
            q.append_pair("romtaille", &query.size.to_string())
                .append_pair("romnom", &query.file_name);
        }
        Ok(url)
    }

    /// Media URLs come from the JSON: only the API host or a sibling host of the same domain is fetched.
    fn media_allowed(&self, media: &Url) -> bool {
        let Ok(base) = Url::parse(&self.base_url) else {
            return false;
        };
        let (Some(base_host), Some(host)) = (base.host_str(), media.host_str()) else {
            return false;
        };
        if host == base_host {
            return true;
        }
        if base_host.parse::<std::net::IpAddr>().is_ok() || base_host.starts_with('[') {
            return false;
        }
        let labels: Vec<&str> = base_host.split('.').collect();
        labels.len() >= 3 && host.ends_with(&format!(".{}", labels[1..].join(".")))
    }
}

impl CoverSource for ScreenScraperSource {
    fn kind(&self) -> CoverSourceKind {
        CoverSourceKind::ScreenScraper
    }

    fn fetch(&mut self, query: &CoverQuery) -> Result<Option<CoverFound>, AssetError> {
        if !self.credentials.is_complete() {
            return Err(AssetError::Unauthorized);
        }
        let Some(system) = query.screenscraper_system else {
            return Ok(None);
        };
        let url = self.info_url(query, system)?;
        let response = self.client.send(&Request {
            method: Method::Get,
            url: url.as_str(),
            headers: Vec::new(),
            body: None,
            max_bytes: MAX_JSON_BYTES,
        })?;
        match response.status {
            200 => {}
            404 => return Ok(None),
            // 401: API closed to non-members; 403: wrong developer credentials; 426: software refused.
            401 | 403 | 426 => return Err(AssetError::Unauthorized),
            // 423: API closed; 429: too many threads; 430: daily quota; 431: unrecognised-ROM quota.
            423 | 429 | 430 | 431 => return Err(AssetError::Quota { retry_after: None }),
            status => return Err(AssetError::Http { status }),
        }
        // serde_json messages may quote the body: not kept.
        let root: SsRoot = serde_json::from_slice(&response.body)
            .map_err(|_| AssetError::InvalidResponse("JSON ScreenScraper illisible".into()))?;
        let medias: Vec<SsMedia> = root
            .response
            .and_then(|r| r.jeu)
            .map(|j| j.medias)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| m.kind == "box-2D" && !m.url.is_empty())
            .collect();
        let rank = |m: &SsMedia| {
            m.region
                .as_deref()
                .and_then(|r| SS_REGIONS.iter().position(|p| *p == r))
                .unwrap_or(SS_REGIONS.len())
        };
        // Stable: equal ranks keep the JSON order ("first" fallback).
        let Some(media) = medias.iter().min_by_key(|m| rank(m)) else {
            return Ok(None);
        };
        let media_url = Url::parse(&media.url)
            .map_err(|_| AssetError::InvalidResponse("URL de média invalide".into()))?;
        if !self.media_allowed(&media_url) {
            tracing::warn!("ScreenScraper : média sur un hôte inattendu, ignoré");
            return Ok(None);
        }
        match fetch_image(&self.client, media_url.as_str(), Vec::new(), self.kind()) {
            Err(AssetError::Http { status: 430 | 431 }) => {
                Err(AssetError::Quota { retry_after: None })
            }
            other => other,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// IGDB

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgdbCredentials {
    pub client_id: String,
    pub client_secret: Secret,
}

impl IgdbCredentials {
    pub fn is_complete(&self) -> bool {
        !(self.client_id.is_empty() || self.client_secret.is_empty())
    }
}

/// IGDB documents 4 requests per second.
const IGDB_INTERVAL: Duration = Duration::from_millis(250);
/// Renewed this long before its announced expiry.
const TOKEN_MARGIN: Duration = Duration::from_secs(60);

/// Twitch client-credentials token (memory only, renewed once on 401), then `POST /v4/games` search by title and
/// platform; ≤ 4 requests/s.
pub struct IgdbSource {
    client: HttpClient,
    credentials: IgdbCredentials,
    token_url: String,
    api_url: String,
    images_url: String,
    token: Option<(Secret, Instant)>,
    last_request: Option<Instant>,
}

#[derive(Deserialize)]
struct TwitchToken {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct IgdbGame {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    cover: Option<IgdbCover>,
}

#[derive(Deserialize)]
struct IgdbCover {
    #[serde(default)]
    image_id: Option<String>,
}

impl IgdbSource {
    pub const TOKEN_URL: &'static str = "https://id.twitch.tv/oauth2/token";
    pub const API_URL: &'static str = "https://api.igdb.com/v4/";
    pub const IMAGES_URL: &'static str = "https://images.igdb.com/igdb/image/upload/t_cover_big/";

    pub fn new(
        client: HttpClient,
        credentials: IgdbCredentials,
        token_url: &str,
        api_url: &str,
        images_url: &str,
    ) -> Self {
        Self {
            client,
            credentials,
            token_url: token_url.to_owned(),
            api_url: with_slash(api_url),
            images_url: with_slash(images_url),
            token: None,
            last_request: None,
        }
    }

    fn throttle(&mut self) {
        if let Some(last) = self.last_request {
            let wait = IGDB_INTERVAL.saturating_sub(last.elapsed());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        self.last_request = Some(Instant::now());
    }

    fn send(&mut self, request: &Request<'_>) -> Result<crate::http::Response, AssetError> {
        self.throttle();
        let response = self.client.send(request)?;
        if response.status == 429 {
            return Err(AssetError::Quota {
                retry_after: Some(response.retry_after.unwrap_or(Duration::from_secs(1))),
            });
        }
        Ok(response)
    }

    fn token(&mut self) -> Result<String, AssetError> {
        if let Some((token, expires)) = &self.token
            && Instant::now() < *expires
        {
            return Ok(token.expose().to_owned());
        }
        self.token = None;
        // Form body, not query string: the secret stays out of the URL.
        let body = format!(
            "client_id={}&client_secret={}&grant_type=client_credentials",
            utf8_percent_encode(&self.credentials.client_id, PATH_SEGMENT),
            utf8_percent_encode(self.credentials.client_secret.expose(), PATH_SEGMENT)
        );
        let token_url = self.token_url.clone();
        let response = self.send(&Request {
            method: Method::Post,
            url: &token_url,
            headers: vec![(
                "Content-Type",
                "application/x-www-form-urlencoded".to_owned(),
            )],
            body: Some(body.into_bytes()),
            max_bytes: MAX_JSON_BYTES,
        })?;
        match response.status {
            200 => {}
            400 | 401 | 403 => return Err(AssetError::Unauthorized),
            status => return Err(AssetError::Http { status }),
        }
        let parsed: TwitchToken = serde_json::from_slice(&response.body)
            .map_err(|_| AssetError::InvalidResponse("jeton Twitch illisible".into()))?;
        if parsed.access_token.is_empty() {
            return Err(AssetError::InvalidResponse("jeton Twitch vide".into()));
        }
        let lifetime =
            Duration::from_secs(parsed.expires_in.unwrap_or(3600)).saturating_sub(TOKEN_MARGIN);
        let token = Secret::new(parsed.access_token);
        let exposed = token.expose().to_owned();
        self.token = Some((token, Instant::now() + lifetime));
        Ok(exposed)
    }

    fn search(&mut self, title: &str, platform: u32) -> Result<Vec<IgdbGame>, AssetError> {
        let body = format!(
            "search \"{title}\"; fields name,cover.image_id; where platforms = ({platform}); limit 5;"
        );
        let url = format!("{}games", self.api_url);
        let mut renewed = false;
        loop {
            let token = self.token()?;
            let response = self.send(&Request {
                method: Method::Post,
                url: &url,
                headers: vec![
                    ("Client-ID", self.credentials.client_id.clone()),
                    ("Authorization", format!("Bearer {token}")),
                    ("Content-Type", "text/plain".to_owned()),
                ],
                body: Some(body.clone().into_bytes()),
                max_bytes: MAX_JSON_BYTES,
            })?;
            match response.status {
                200 => {
                    return serde_json::from_slice(&response.body)
                        .map_err(|_| AssetError::InvalidResponse("réponse IGDB illisible".into()));
                }
                401 if !renewed => {
                    renewed = true;
                    self.token = None;
                }
                status => return Err(status_error(status, response.retry_after)),
            }
        }
    }
}

/// `"Sonic (USA) [b]"` → `Sonic`: DAT tags hurt a text search. Quotes and backslashes cannot be escaped in
/// IGDB's query language: removed, like control characters.
fn igdb_search_title(title: &str) -> String {
    let end = [" (", " ["]
        .iter()
        .filter_map(|p| title.find(p))
        .min()
        .unwrap_or(title.len());
    title[..end]
        .chars()
        .filter(|c| !matches!(c, '"' | '\\') && !c.is_control())
        .take(200)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn valid_image_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl CoverSource for IgdbSource {
    fn kind(&self) -> CoverSourceKind {
        CoverSourceKind::Igdb
    }

    fn fetch(&mut self, query: &CoverQuery) -> Result<Option<CoverFound>, AssetError> {
        if !self.credentials.is_complete() {
            return Err(AssetError::Unauthorized);
        }
        let Some(platform) = query.igdb_platform else {
            return Ok(None);
        };
        let title = igdb_search_title(&query.title);
        if title.is_empty() {
            return Ok(None);
        }
        let games = self.search(&title, platform)?;
        let with_cover = || {
            games.iter().filter_map(|g| {
                let id = g.cover.as_ref()?.image_id.as_deref()?;
                valid_image_id(id).then_some((g, id))
            })
        };
        let exact = with_cover().find(|(g, _)| {
            g.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(&title))
        });
        let Some((_, image_id)) = exact.or_else(|| with_cover().next()) else {
            return Ok(None);
        };
        let url = format!("{}{image_id}.jpg", self.images_url);
        self.throttle();
        fetch_image(&self.client, &url, Vec::new(), self.kind())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Chain

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Off {
    /// Transient errors in a row: counts towards `offline()`.
    Network,
    /// Session quota: the next session may succeed, so absences are not conclusive.
    Quota,
    /// Credentials refused or URL refused: the source is out until the user changes the configuration.
    Refused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    Active,
    Suspended(Instant),
    Off(Off),
}

struct Slot {
    source: Box<dyn CoverSource>,
    state: SlotState,
    network_errors: u32,
}

/// Sources in user order. A source failing with `Unauthorized` / `Quota`, or 3 `Network` errors in a row, is
/// skipped until `reset`; when every source is skipped for network reasons, `offline()` is true.
///
/// `Quota` with a `Retry-After` (IGDB) only suspends the source for that long. Unusable content (invalid image or
/// JSON, oversized body, other 4xx) counts as "not found" for that source, so a broken entry is not retried at
/// every pass; a source whose credentials were refused does not prevent a conclusive `Ok(None)`.
pub struct CoverChain {
    slots: Vec<Slot>,
}

impl CoverChain {
    pub fn new(sources: Vec<Box<dyn CoverSource>>) -> Self {
        Self {
            slots: sources
                .into_iter()
                .map(|source| Slot {
                    source,
                    state: SlotState::Active,
                    network_errors: 0,
                })
                .collect(),
        }
    }

    /// `Ok(None)`: every active source answered "not found" (the caller records `CoverState::Missing`).
    /// `Err`: nothing conclusive (offline, quota, no source): nothing must be recorded.
    pub fn fetch(&mut self, query: &CoverQuery) -> Result<Option<CoverFound>, AssetError> {
        let mut consulted = false;
        let mut inconclusive: Option<AssetError> = None;
        for slot in &mut self.slots {
            let name = slot.source.kind().name();
            match slot.state {
                SlotState::Off(Off::Refused) => continue,
                SlotState::Off(Off::Network) => {
                    inconclusive.get_or_insert(AssetError::Network("hors ligne".into()));
                    continue;
                }
                SlotState::Off(Off::Quota) => {
                    inconclusive.get_or_insert(AssetError::Quota { retry_after: None });
                    continue;
                }
                SlotState::Suspended(until) => {
                    let now = Instant::now();
                    if now < until {
                        inconclusive.get_or_insert(AssetError::Quota {
                            retry_after: Some(until - now),
                        });
                        continue;
                    }
                    slot.state = SlotState::Active;
                }
                SlotState::Active => {}
            }
            match slot.source.fetch(query) {
                Ok(Some(found)) => {
                    slot.network_errors = 0;
                    return Ok(Some(found));
                }
                Ok(None) => {
                    slot.network_errors = 0;
                    consulted = true;
                }
                Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
                Err(e @ (AssetError::Unauthorized | AssetError::UrlRefused)) => {
                    tracing::warn!("jaquettes : source {name} désactivée : {e}");
                    slot.state = SlotState::Off(Off::Refused);
                }
                Err(AssetError::Quota { retry_after }) => {
                    match retry_after {
                        Some(delay) => {
                            let delay = delay.min(MAX_SUSPEND);
                            tracing::info!(
                                "jaquettes : source {name} en pause {}",
                                delay.as_secs()
                            );
                            slot.state = SlotState::Suspended(Instant::now() + delay);
                        }
                        None => {
                            tracing::warn!(
                                "jaquettes : quota {name} atteint, source coupée pour la session"
                            );
                            slot.state = SlotState::Off(Off::Quota);
                        }
                    }
                    inconclusive = Some(AssetError::Quota { retry_after });
                }
                Err(e) if e.is_transient() || matches!(e, AssetError::Io { .. }) => {
                    slot.network_errors += 1;
                    tracing::debug!("jaquettes : {name} : {e}");
                    if slot.network_errors >= MAX_NETWORK_ERRORS {
                        tracing::warn!(
                            "jaquettes : source {name} injoignable, suspendue pour la session"
                        );
                        slot.state = SlotState::Off(Off::Network);
                    }
                    inconclusive = Some(e);
                }
                Err(e) => {
                    tracing::warn!("jaquettes : {name} : {e} (considérée absente)");
                    slot.network_errors = 0;
                    consulted = true;
                }
            }
        }
        if self.offline() {
            return Err(AssetError::Network("hors ligne".into()));
        }
        match inconclusive {
            Some(e) => Err(e),
            None if consulted => Ok(None),
            None => Err(AssetError::NoSource),
        }
    }

    /// Every source is out and at least one for network reasons ("hors ligne" indicator).
    pub fn offline(&self) -> bool {
        !self.slots.is_empty()
            && self
                .slots
                .iter()
                .all(|s| matches!(s.state, SlotState::Off(_)))
            && self
                .slots
                .iter()
                .any(|s| s.state == SlotState::Off(Off::Network))
    }

    /// Re-enables every source (« Rechercher à nouveau », network back, credentials changed).
    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.state = SlotState::Active;
            slot.network_errors = 0;
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Cache

/// Default bound of [`CoverCache`] (`with_limit` to change it).
pub const DEFAULT_COVER_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

/// `<data>/covers/<system id>/boxart/<key>.<ext>`, `key` = SHA1 in hex (else a hash of the path).
///
/// Bounded: when a `store` takes the total past the limit, the oldest files (by modification time) are deleted
/// down to 90 % of it. The database may then point to a deleted file: a missing or unreadable cached file must
/// be treated as "not cached" (download again).
#[derive(Debug, Clone)]
pub struct CoverCache {
    pub root: PathBuf,
    max_bytes: u64,
    /// Known total size; `None` until the first `store` walks the tree. Shared by clones.
    used: Arc<Mutex<Option<u64>>>,
}

impl PartialEq for CoverCache {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root && self.max_bytes == other.max_bytes
    }
}

impl Eq for CoverCache {}

const BOXART: &str = "boxart";

fn safe_component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl CoverCache {
    pub fn new(root: &Path) -> Self {
        Self::with_limit(root, DEFAULT_COVER_CACHE_BYTES)
    }

    pub fn with_limit(root: &Path, max_bytes: u64) -> Self {
        Self {
            root: root.to_path_buf(),
            max_bytes,
            used: Arc::new(Mutex::new(None)),
        }
    }

    /// Atomic write (temporary file + rename). Returns the path relative to `root` (stored in the database).
    /// `system` and `key` must be `[A-Za-z0-9_-]{1,128}`; the bytes must match `found.format`.
    pub fn store(
        &self,
        system: &str,
        key: &str,
        found: &CoverFound,
    ) -> Result<PathBuf, AssetError> {
        let relative = Path::new(system).join(BOXART);
        let dir = self.root.join(&relative);
        if !safe_component(system) || !safe_component(key) {
            return Err(AssetError::Io {
                path: dir,
                source: std::io::Error::from(std::io::ErrorKind::InvalidInput),
            });
        }
        if found.bytes.len() as u64 > MAX_COVER_BYTES
            || sniff_format(&found.bytes) != Some(found.format)
        {
            return Err(AssetError::InvalidImage("contenu incohérent".into()));
        }
        let file = format!("{key}.{}", found.format.extension());
        let dest = dir.join(&file);
        write_atomic(&dest, &found.bytes)?;
        // Same key, other format: stale.
        for other in [ImageFormat::Png, ImageFormat::Jpeg] {
            if other != found.format {
                let _ = std::fs::remove_file(dir.join(format!("{key}.{}", other.extension())));
            }
        }
        self.account(found.bytes.len() as u64, &dest);
        Ok(relative.join(file))
    }

    /// Refuses relative paths escaping `root` (`..`, absolute): the database is not trusted either.
    /// No file system access (usable from the UI thread): the file may not exist.
    pub fn resolve(&self, relative: &Path) -> Option<PathBuf> {
        let mut components = 0;
        for component in relative.components() {
            match component {
                Component::Normal(_) => components += 1,
                _ => return None,
            }
        }
        (components > 0).then(|| self.root.join(relative))
    }

    fn account(&self, added: u64, keep: &Path) {
        let mut used = self.used.lock().unwrap_or_else(PoisonError::into_inner);
        let total = match *used {
            Some(total) => total.saturating_add(added),
            None => self.files().iter().map(|f| f.1).sum(),
        };
        *used = Some(if total > self.max_bytes {
            self.prune(keep)
        } else {
            total
        });
    }

    /// `(mtime, size, path)` of every regular file under `<root>/*/boxart/`.
    fn files(&self) -> Vec<(SystemTime, u64, PathBuf)> {
        let mut files = Vec::new();
        let Ok(systems) = std::fs::read_dir(&self.root) else {
            return files;
        };
        for system in systems.filter_map(Result::ok) {
            let Ok(entries) = std::fs::read_dir(system.path().join(BOXART)) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                // `DirEntry::metadata` does not follow symbolic links.
                if let Ok(meta) = entry.metadata()
                    && meta.is_file()
                {
                    let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    files.push((mtime, meta.len(), entry.path()));
                }
            }
        }
        files
    }

    /// Deletes the oldest files down to 90 % of the limit; returns the new total.
    fn prune(&self, keep: &Path) -> u64 {
        let mut files = self.files();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        let target = self.max_bytes / 10 * 9;
        files.sort();
        for (_, size, path) in files {
            if total <= target {
                break;
            }
            if path == keep {
                continue;
            }
            match std::fs::remove_file(&path) {
                Ok(()) => total -= size,
                Err(e) => tracing::warn!("cache de jaquettes : {} : {e}", path.display()),
            }
        }
        total
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests;
