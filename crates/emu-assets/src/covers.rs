//! Cover sources (ADR 0014 §5) and the disk cache. No threads here: the queue lives in `emu-ui`, which owns the
//! database side (`emu-library`). Every call is blocking: worker threads only.
//!
//! Credentials never reach a log line or an error: ScreenScraper puts them in query strings (its media URLs too),
//! so no URL built or received here is ever logged; `AssetError` carries no URL by construction.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use zeroize::{Zeroize, Zeroizing};

use percent_encoding::utf8_percent_encode;
use reqwest::Url;
use serde::Deserialize;

use crate::dat::{PATH_SEGMENT, write_atomic};
use crate::http::{CancelToken, HttpClient, Method, Request};
use crate::{AssetError, ImageFormat, MAX_COVER_BYTES, Secret, sniff_format};

/// JSON answers (ScreenScraper `jeuInfos`, IGDB search, Twitch token).
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;
/// Longest `Retry-After` honoured; a longer one suspends the source for that long only.
const MAX_SUSPEND: Duration = Duration::from_secs(3600);
/// ScreenScraper 429 ("too many threads for this account") without `Retry-After`: a short pause, not a cut.
const SS_THREADS_PAUSE: Duration = Duration::from_secs(5);
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
    /// Forgets the session state (« Rechercher à nouveau »); called by [`CoverChain::reset`].
    fn reset(&mut self) {}
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

/// GET of an image, then validation (signature, limits, decode without downscaling). `404` = absent.
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
    let format = crate::image::validate_cover(&response.body)?;
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

/// Consecutive 404 of identified games without any success after which a mirror is judged broken for the session
/// (ADR 0014, 7.0.2), provided they span at least [`BROKEN_MIN_SYSTEMS`] systems.
const BROKEN_AFTER_404: u32 = 8;
/// Distinct systems the consecutive identified 404 must cover; unidentified games (TOSEC names of computers) answer
/// 404 legitimately and never count.
const BROKEN_MIN_SYSTEMS: usize = 3;
/// Simultaneous requests allowed on `raw.githubusercontent.com` (all `LibretroThumbnails` of the process).
const RAW_MAX_CONCURRENT: u32 = 2;

struct RawGate {
    busy: Mutex<u32>,
    freed: Condvar,
}

static RAW_GATE: RawGate = RawGate {
    busy: Mutex::new(0),
    freed: Condvar::new(),
};

struct RawPermit;

impl RawPermit {
    fn acquire() -> Self {
        let mut busy = RAW_GATE.busy.lock().unwrap_or_else(PoisonError::into_inner);
        while *busy >= RAW_MAX_CONCURRENT {
            busy = RAW_GATE
                .freed
                .wait(busy)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *busy += 1;
        Self
    }
}

impl Drop for RawPermit {
    fn drop(&mut self) {
        let mut busy = RAW_GATE.busy.lock().unwrap_or_else(PoisonError::into_inner);
        *busy -= 1;
        RAW_GATE.freed.notify_one();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MirrorState {
    Active,
    /// 403/429: no insistence for the session.
    Stopped,
    /// `BROKEN_AFTER_404` consecutive 404 of identified games over `BROKEN_MIN_SYSTEMS` systems, no success.
    Broken,
}

enum Attempt {
    Found(CoverFound),
    /// 404 from a healthy mirror (or unusable image content).
    Absent,
    Inconclusive(AssetError),
    Stopped,
    Broken,
}

/// The only hosts allowed for covers (HTTPS only); loopback HTTP stays possible for tests, the client refusing it
/// unless configured so.
fn mirror_base_allowed(base: &str) -> bool {
    let Ok(url) = Url::parse(base) else {
        return false;
    };
    match url.scheme() {
        "https" => matches!(
            url.host_str(),
            Some("raw.githubusercontent.com" | "thumbnails.libretro.com")
        ),
        "http" => url.host_str() == Some("127.0.0.1"),
        _ => false,
    }
}

struct Mirror {
    /// `None`: base refused (host not allowed); the mirror is never asked.
    base: Option<String>,
    raw: bool,
    /// Consecutive 404 of identified games since the last success.
    misses: u32,
    /// Databases of those 404.
    miss_systems: HashSet<String>,
    state: MirrorState,
}

impl Mirror {
    fn new(base: &str, raw: bool) -> Self {
        Self {
            base: mirror_base_allowed(base).then(|| with_slash(base)),
            raw,
            misses: 0,
            miss_systems: HashSet::new(),
            state: MirrorState::Active,
        }
    }

    fn reset_misses(&mut self) {
        self.misses = 0;
        self.miss_systems.clear();
    }

    fn url(&self, base: &str, query: &CoverQuery) -> String {
        let name = thumbnail_file_name(&query.title);
        let file = utf8_percent_encode(&name, PATH_SEGMENT);
        if self.raw {
            // Repository name = libretro database with spaces turned into underscores.
            let repo = query.database.replace(' ', "_");
            format!(
                "{base}{}/master/Named_Boxarts/{file}",
                utf8_percent_encode(&repo, PATH_SEGMENT)
            )
        } else {
            format!(
                "{base}{}/Named_Boxarts/{file}",
                utf8_percent_encode(&query.database, PATH_SEGMENT)
            )
        }
    }

    fn name(&self) -> &'static str {
        if self.raw {
            "libretro (miroir GitHub)"
        } else {
            "libretro (thumbnails.libretro.com)"
        }
    }

    fn attempt(&mut self, client: &HttpClient, query: &CoverQuery) -> Attempt {
        let Some(base) = self.base.clone() else {
            return Attempt::Inconclusive(AssetError::UrlRefused);
        };
        match self.state {
            MirrorState::Broken => return Attempt::Broken,
            MirrorState::Stopped => return Attempt::Stopped,
            MirrorState::Active => {}
        }
        let url = self.url(&base, query);
        let _permit = self.raw.then(RawPermit::acquire);
        let response = match client.send(&Request {
            method: Method::Get,
            url: &url,
            headers: Vec::new(),
            body: None,
            max_bytes: MAX_COVER_BYTES,
        }) {
            Ok(r) => r,
            Err(e) => return Attempt::Inconclusive(e),
        };
        match response.status {
            200 => {
                self.reset_misses();
                match crate::image::validate_cover(&response.body) {
                    Ok(format) => Attempt::Found(CoverFound {
                        source: CoverSourceKind::LibretroThumbnails,
                        format,
                        bytes: response.body,
                    }),
                    // The chain counts unusable content as "not found" for the source.
                    Err(e) => Attempt::Inconclusive(e),
                }
            }
            404 | 410 => {
                if query.identified {
                    self.misses += 1;
                    self.miss_systems.insert(query.database.clone());
                }
                if self.misses >= BROKEN_AFTER_404 && self.miss_systems.len() >= BROKEN_MIN_SYSTEMS
                {
                    tracing::warn!(
                        "jaquettes : {} : {} réponses 404 sans succès, source jugée en panne pour la session",
                        self.name(),
                        self.misses
                    );
                    self.state = MirrorState::Broken;
                    Attempt::Broken
                } else {
                    Attempt::Absent
                }
            }
            403 | 429 => {
                tracing::warn!(
                    "jaquettes : {} : HTTP {}, source arrêtée pour la session",
                    self.name(),
                    response.status
                );
                self.state = MirrorState::Stopped;
                Attempt::Stopped
            }
            // 3xx included: redirections are never followed, so they prove nothing.
            status => Attempt::Inconclusive(AssetError::Http { status }),
        }
    }
}

/// libretro-thumbnails: the GitHub mirror
/// (`https://raw.githubusercontent.com/libretro-thumbnails/<system with _>/master/Named_Boxarts/<name>.png`)
/// first, `https://thumbnails.libretro.com/<system>/Named_Boxarts/<name>.png` as a fallback. No account.
///
/// Per mirror: 404 is "absent" only while it is healthy; [`BROKEN_AFTER_404`] consecutive 404 of identified games
/// over at least [`BROKEN_MIN_SYSTEMS`] systems, without a success, make it "broken" for the session (`AssetError::SourceBroken`, nothing conclusive); 403/429 stop it; 3xx is
/// inconclusive; at most 2 simultaneous requests on the GitHub host.
pub struct LibretroThumbnails {
    client: HttpClient,
    mirrors: [Mirror; 2],
}

impl LibretroThumbnails {
    /// Fallback host (`thumbnails.libretro.com`).
    pub const BASE_URL: &'static str = "https://thumbnails.libretro.com/";
    /// Mirror asked first.
    pub const RAW_BASE_URL: &'static str = "https://raw.githubusercontent.com/libretro-thumbnails/";

    /// Production mirrors, with `base_url` as the fallback host.
    pub fn new(client: HttpClient, base_url: &str) -> Self {
        Self::with_bases(client, Self::RAW_BASE_URL, base_url)
    }

    pub fn with_bases(client: HttpClient, raw_base_url: &str, fallback_base_url: &str) -> Self {
        Self {
            client: client.without_redirects(),
            mirrors: [
                Mirror::new(raw_base_url, true),
                Mirror::new(fallback_base_url, false),
            ],
        }
    }

    /// Fallback URL (thumbnails.libretro.com layout).
    pub fn url(&self, query: &CoverQuery) -> String {
        self.mirror_url(1, query)
    }

    /// GitHub mirror URL.
    pub fn raw_url(&self, query: &CoverQuery) -> String {
        self.mirror_url(0, query)
    }

    fn mirror_url(&self, index: usize, query: &CoverQuery) -> String {
        let mirror = &self.mirrors[index];
        mirror.url(mirror.base.as_deref().unwrap_or_default(), query)
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
        let (mut absent, mut broken, mut stopped) = (false, false, false);
        let mut inconclusive: Option<AssetError> = None;
        for mirror in &mut self.mirrors {
            match mirror.attempt(&self.client, query) {
                Attempt::Found(found) => return Ok(Some(found)),
                Attempt::Absent => absent = true,
                Attempt::Broken => broken = true,
                Attempt::Stopped => stopped = true,
                Attempt::Inconclusive(AssetError::Cancelled) => return Err(AssetError::Cancelled),
                Attempt::Inconclusive(e) => {
                    inconclusive.get_or_insert(e);
                }
            }
        }
        if let Some(e) = inconclusive {
            Err(e)
        } else if absent {
            Ok(None)
        } else if broken {
            Err(AssetError::SourceBroken)
        } else if stopped {
            Err(AssetError::Quota { retry_after: None })
        } else {
            Err(AssetError::UrlRefused)
        }
    }

    fn reset(&mut self) {
        for mirror in &mut self.mirrors {
            mirror.reset_misses();
            mirror.state = MirrorState::Active;
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// ScreenScraper

#[derive(Clone, PartialEq, Eq)]
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

/// Only says whether each field is set (the user name and developer id are personal too).
impl std::fmt::Debug for ScreenScraperCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenScraperCredentials")
            .field("dev_id", &set(self.dev_id.is_empty()))
            .field("dev_password", &set(self.dev_password.is_empty()))
            .field("user", &set(self.user.is_empty()))
            .field("password", &set(self.password.is_empty()))
            .finish()
    }
}

fn set(empty: bool) -> &'static str {
    if empty { "non défini" } else { "défini" }
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

    /// Fetches the chosen media. Its URL comes from the JSON, so a refusal or a 4xx there means "this entry
    /// has no usable cover", never "the source is unusable": only quotas stop the source.
    fn fetch_media(&self, media_url: &Url) -> Result<Option<CoverFound>, AssetError> {
        match fetch_image(&self.client, media_url.as_str(), Vec::new(), self.kind()) {
            Err(AssetError::Http { status: 430 | 431 }) => {
                Err(AssetError::Quota { retry_after: None })
            }
            Err(AssetError::Quota { retry_after }) => Err(AssetError::Quota {
                retry_after: Some(retry_after.unwrap_or(SS_THREADS_PAUSE)),
            }),
            Err(e @ (AssetError::UrlRefused | AssetError::Unauthorized)) => {
                tracing::debug!("ScreenScraper : média refusé ({e}), considéré absent");
                Ok(None)
            }
            Err(AssetError::Http { status }) if (400..500).contains(&status) => {
                tracing::debug!("ScreenScraper : média en HTTP {status}, considéré absent");
                Ok(None)
            }
            other => other,
        }
    }
}

/// Media URLs come from the JSON: HTTPS only, on the API host or a sibling host of the same registrable domain
/// (`api.screenscraper.fr` → `*.screenscraper.fr`). Without a public suffix list, a parent domain made of a short
/// label under a two-letter TLD (`co.uk`, `com.au`) is taken as a public suffix and grants no sibling.
/// Plain `http` is accepted only on the API host itself when the API is `http` (loopback tests).
fn media_allowed(base: &Url, media: &Url) -> bool {
    let (Some(base_host), Some(host)) = (base.host_str(), media.host_str()) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let base_host = base_host.to_ascii_lowercase();
    if media.scheme() != "https" {
        return media.scheme() == base.scheme()
            && host == base_host
            && media.port_or_known_default() == base.port_or_known_default();
    }
    if host == base_host {
        return true;
    }
    if base_host.parse::<std::net::IpAddr>().is_ok() || base_host.starts_with('[') {
        return false;
    }
    let labels: Vec<&str> = base_host.split('.').collect();
    if labels.len() < 3 || labels.iter().any(|l| l.is_empty()) {
        return false;
    }
    let parent = &labels[1..];
    let looks_public = parent.len() == 2 && parent[0].len() <= 3 && parent[1].len() == 2;
    !looks_public && host.ends_with(&format!(".{}", parent.join(".")))
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
            // 429: too many threads for the account, gone in a few seconds.
            429 => {
                return Err(AssetError::Quota {
                    retry_after: Some(response.retry_after.unwrap_or(SS_THREADS_PAUSE)),
                });
            }
            // 423: API closed; 430: daily quota; 431: unrecognised-ROM quota.
            423 | 430 | 431 => return Err(AssetError::Quota { retry_after: None }),
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
        let allowed = Url::parse(&self.base_url).is_ok_and(|base| media_allowed(&base, &media_url));
        if !allowed {
            tracing::warn!("ScreenScraper : média sur un hôte inattendu, ignoré");
            return Ok(None);
        }
        self.fetch_media(&media_url)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// IGDB

#[derive(Clone, PartialEq, Eq)]
pub struct IgdbCredentials {
    pub client_id: String,
    pub client_secret: Secret,
}

impl std::fmt::Debug for IgdbCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IgdbCredentials")
            .field("client_id", &set(self.client_id.is_empty()))
            .field("client_secret", &set(self.client_secret.is_empty()))
            .finish()
    }
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

    /// Sends a request carrying secrets (headers, body), then wipes its buffers. Best effort only: `reqwest`,
    /// `hyper` and `rustls` copy headers and body into their own buffers (header maps, write buffers, TLS records
    /// before encryption) and free them without wiping, so plaintext copies may linger in freed heap memory until
    /// reused. Same for the response body (IGDB access token) before it is moved into a `Secret`.
    fn send_secret(
        &mut self,
        mut request: Request<'_>,
    ) -> Result<crate::http::Response, AssetError> {
        let result = self.send(&request);
        for (_, value) in &mut request.headers {
            value.zeroize();
        }
        if let Some(body) = &mut request.body {
            body.zeroize();
        }
        result
    }

    fn token(&mut self) -> Result<Zeroizing<String>, AssetError> {
        if let Some((token, expires)) = &self.token
            && Instant::now() < *expires
        {
            return Ok(Zeroizing::new(token.expose().to_owned()));
        }
        self.token = None;
        // Form body, not query string: the secret stays out of the URL.
        // Sized up front (percent-encoding at most triples a byte): `write!` never reallocates, so no copy of
        // the secret is left behind in a freed, unwiped buffer.
        let client_id = &self.credentials.client_id;
        let secret = self.credentials.client_secret.expose();
        let mut body = Zeroizing::new(String::with_capacity(
            64 + 3 * (client_id.len() + secret.len()),
        ));
        let _ = write!(
            body,
            "client_id={}&client_secret={}&grant_type=client_credentials",
            utf8_percent_encode(client_id, PATH_SEGMENT),
            utf8_percent_encode(secret, PATH_SEGMENT)
        );
        let token_url = self.token_url.clone();
        let response = self.send_secret(Request {
            method: Method::Post,
            url: &token_url,
            headers: vec![(
                "Content-Type",
                "application/x-www-form-urlencoded".to_owned(),
            )],
            body: Some(body.as_bytes().to_vec()),
            max_bytes: MAX_JSON_BYTES,
        })?;
        let status = response.status;
        let response_body = Zeroizing::new(response.body);
        match status {
            200 => {}
            400 | 401 | 403 => return Err(AssetError::Unauthorized),
            status => return Err(AssetError::Http { status }),
        }
        let parsed: TwitchToken = serde_json::from_slice(&response_body)
            .map_err(|_| AssetError::InvalidResponse("jeton Twitch illisible".into()))?;
        let expires_in = parsed.expires_in;
        // Moved, not copied, into the `Secret`, which wipes it on drop.
        let token = Secret::new(parsed.access_token);
        if token.is_empty() {
            return Err(AssetError::InvalidResponse("jeton Twitch vide".into()));
        }
        let lifetime = Duration::from_secs(expires_in.unwrap_or(3600)).saturating_sub(TOKEN_MARGIN);
        let exposed = Zeroizing::new(token.expose().to_owned());
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
            let client_id = self.credentials.client_id.clone();
            let response = self.send_secret(Request {
                method: Method::Post,
                url: &url,
                headers: vec![
                    ("Client-ID", client_id),
                    ("Authorization", format!("Bearer {}", token.as_str())),
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
    /// `AssetError::SourceBroken`: only a `reset` brings it back.
    Broken,
}

/// Session counters of a [`CoverChain`] (one count per `fetch` outcome).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CoverStats {
    /// Covers downloaded.
    pub found: u32,
    /// Conclusive « absent » answers (recorded `Missing` by the caller).
    pub missing: u32,
    /// Inconclusive failures (network, quota, HTTP, cancelled excluded).
    pub errors: u32,
    /// A source was judged broken during the session (`AssetError::SourceBroken`); cleared by `reset`.
    pub source_broken: bool,
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
/// `Quota` with a delay (IGDB `Retry-After`, ScreenScraper 429) only suspends the source for that long. Unusable content (invalid image or
/// JSON, oversized body, other 4xx) counts as "not found" for that source, so a broken entry is not retried at
/// every pass; a source whose credentials were refused does not prevent a conclusive `Ok(None)`.
pub struct CoverChain {
    slots: Vec<Slot>,
    stats: CoverStats,
}

impl CoverChain {
    pub fn new(sources: Vec<Box<dyn CoverSource>>) -> Self {
        Self {
            stats: CoverStats::default(),
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
        self.fetch_bounded(query, None, None)
    }

    /// Same as [`Self::fetch`], bounded for the UI: nothing new is started once `budget` (counted from this
    /// call, whole chain) has elapsed or `cancel` is set.
    ///
    /// - `cancel` set: `Err(Cancelled)`, nothing recorded.
    /// - Budget spent before every source was asked: `Err(Network("délai dépassé"))`, never `Ok(None)` (the
    ///   unasked sources might know the cover), and it does not count toward a source's 3-error shutdown.
    ///
    /// A request already in flight is not interrupted: the real worst case is `budget` plus one request
    /// (`HttpConfig::timeout`), so lower that timeout (e.g. 10 s) to hold a ~30 s total.
    pub fn fetch_with_deadline(
        &mut self,
        query: &CoverQuery,
        budget: Duration,
        cancel: Option<&CancelToken>,
    ) -> Result<Option<CoverFound>, AssetError> {
        self.fetch_bounded(query, Some(Instant::now() + budget), cancel)
    }

    /// Session counters (found, missing, errors, broken source).
    pub fn stats(&self) -> CoverStats {
        self.stats
    }

    /// Zeroes the counters (the sources keep their state: see `reset`).
    pub fn reset_stats(&mut self) {
        self.stats = CoverStats::default();
    }

    fn fetch_bounded(
        &mut self,
        query: &CoverQuery,
        deadline: Option<Instant>,
        cancel: Option<&CancelToken>,
    ) -> Result<Option<CoverFound>, AssetError> {
        let result = self.fetch_inner(query, deadline, cancel);
        match &result {
            Ok(Some(_)) => self.stats.found += 1,
            Ok(None) => self.stats.missing += 1,
            Err(AssetError::Cancelled) => {}
            Err(AssetError::SourceBroken) => self.stats.source_broken = true,
            Err(_) => self.stats.errors += 1,
        }
        result
    }

    fn fetch_inner(
        &mut self,
        query: &CoverQuery,
        deadline: Option<Instant>,
        cancel: Option<&CancelToken>,
    ) -> Result<Option<CoverFound>, AssetError> {
        let mut consulted = false;
        let mut inconclusive: Option<AssetError> = None;
        for slot in &mut self.slots {
            if cancel.is_some_and(CancelToken::is_cancelled) {
                return Err(AssetError::Cancelled);
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Err(
                    inconclusive.unwrap_or_else(|| AssetError::Network("délai dépassé".into()))
                );
            }
            let name = slot.source.kind().name();
            match slot.state {
                SlotState::Off(Off::Refused) => continue,
                SlotState::Off(Off::Network) => {
                    inconclusive.get_or_insert(AssetError::Network("hors ligne".into()));
                    continue;
                }
                SlotState::Off(Off::Broken) => {
                    inconclusive = Some(AssetError::SourceBroken);
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
                Err(AssetError::SourceBroken) => {
                    tracing::warn!("jaquettes : source {name} en panne, sautée pour la session");
                    slot.state = SlotState::Off(Off::Broken);
                    inconclusive = Some(AssetError::SourceBroken);
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
                // Redirections are never followed: they prove nothing about the cover.
                Err(e @ AssetError::Http { status: 300..=399 }) => {
                    tracing::warn!("jaquettes : {name} : {e} (redirection ignorée)");
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
            slot.source.reset();
        }
        self.stats.source_broken = false;
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
const STALE_TEMPORARY: Duration = Duration::from_secs(3600);

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
        // A rewritten file replaces its previous size instead of adding to it.
        let mut removed = regular_file_size(&dest);
        write_atomic(&dest, &found.bytes)?;
        // Same key, other format: stale.
        for other in [ImageFormat::Png, ImageFormat::Jpeg] {
            if other != found.format {
                let stale = dir.join(format!("{key}.{}", other.extension()));
                let size = regular_file_size(&stale);
                if size > 0 && std::fs::remove_file(&stale).is_ok() {
                    removed += size;
                }
            }
        }
        self.account(found.bytes.len() as u64, removed, &dest);
        Ok(relative.join(file))
    }

    /// Deletes one cached file, given the relative path stored in the database. `Ok(true)` if a file was
    /// deleted, `Ok(false)` if nothing was there (or it is not a regular file: directories and symbolic links
    /// are never touched). A path escaping `root` (`..`, absolute, drive prefix) is refused with an
    /// `AssetError::Io` of kind `InvalidInput`. Blocking file system call: not for the UI thread.
    pub fn remove(&self, relative: &Path) -> Result<bool, AssetError> {
        let Some(path) = self.resolve(relative) else {
            return Err(AssetError::Io {
                path: relative.to_path_buf(),
                source: std::io::Error::from(std::io::ErrorKind::InvalidInput),
            });
        };
        let size = regular_file_size(&path);
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            return Ok(false);
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(source) => return Err(AssetError::Io { path, source }),
        }
        let mut used = self.used.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(total) = used.as_mut() {
            *total = total.saturating_sub(size);
        }
        Ok(true)
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

    fn account(&self, added: u64, removed: u64, keep: &Path) {
        let mut used = self.used.lock().unwrap_or_else(PoisonError::into_inner);
        let total = match *used {
            Some(total) => total.saturating_sub(removed).saturating_add(added),
            None => self.files().iter().map(|f| f.1).sum(),
        };
        *used = Some(if total > self.max_bytes {
            self.prune(keep)
        } else {
            total
        });
    }

    /// `(mtime, size, path)` of every regular file under `<root>/*/boxart/`. Symbolic links are never followed
    /// (neither directories nor files: pruning must not delete outside the cache), and in-flight temporaries of
    /// `write_atomic` (`.<name>.tmp-*`) are skipped.
    fn files(&self) -> Vec<(SystemTime, u64, PathBuf)> {
        self.walk().0
    }

    /// [`Self::files`], plus the `write_atomic` temporaries as `(mtime, path)`.
    #[allow(clippy::type_complexity)]
    fn walk(&self) -> (Vec<(SystemTime, u64, PathBuf)>, Vec<(SystemTime, PathBuf)>) {
        let mut files = Vec::new();
        let mut temporaries = Vec::new();
        let Ok(systems) = std::fs::read_dir(&self.root) else {
            return (files, temporaries);
        };
        for system in systems.filter_map(Result::ok) {
            // `DirEntry::file_type` does not follow symbolic links.
            if !system.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let boxart = system.path().join(BOXART);
            if !std::fs::symlink_metadata(&boxart).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&boxart) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                if !entry.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if is_temporary(&entry.file_name()) {
                    temporaries.push((mtime, entry.path()));
                } else {
                    files.push((mtime, meta.len(), entry.path()));
                }
            }
        }
        (files, temporaries)
    }

    /// Deletes the oldest files down to 90 % of the limit; returns the new total. Temporaries older than
    /// [`STALE_TEMPORARY`] (left by a crash between write and rename) are deleted too; younger ones may still
    /// be in flight in another thread or process.
    fn prune(&self, keep: &Path) -> u64 {
        let (mut files, temporaries) = self.walk();
        let now = SystemTime::now();
        for (mtime, path) in temporaries {
            if now
                .duration_since(mtime)
                .is_ok_and(|age| age > STALE_TEMPORARY)
                && let Err(e) = std::fs::remove_file(&path)
            {
                tracing::warn!("cache de jaquettes : {} : {e}", path.display());
            }
        }
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

/// Size of `path` if it is a regular file (not followed when a symbolic link), else 0.
fn regular_file_size(path: &Path) -> u64 {
    std::fs::symlink_metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map_or(0, |m| m.len())
}

/// `write_atomic` temporaries: `.<file name>.tmp-<random>`.
fn is_temporary(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|n| n.starts_with('.') && n.contains(".tmp-"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests;
