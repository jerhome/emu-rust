use std::path::PathBuf;
use std::time::Duration;

/// Never contains a URL, a header or a response body: they may carry credentials (ScreenScraper puts them in the
/// query string). Messages are user-facing French.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AssetError {
    /// DNS, connection, TLS or timeout (`reqwest::Error::without_url`).
    #[error("réseau : {0}")]
    Network(String),
    #[error("réponse HTTP {status}")]
    Http { status: u16 },
    #[error("réponse trop volumineuse (limite {limit} octets)")]
    TooLarge { limit: u64 },
    /// Wrong or missing credentials (the source is disabled for the session).
    #[error("identifiants refusés")]
    Unauthorized,
    /// Quota or rate limit reached; `retry_after` when the server said so.
    #[error("quota atteint")]
    Quota { retry_after: Option<Duration> },
    #[error("réponse invalide : {0}")]
    InvalidResponse(String),
    #[error("image invalide : {0}")]
    InvalidImage(String),
    /// Only `https://` (and `http://127.0.0.1` when `HttpConfig::allow_loopback_http`).
    #[error("adresse refusée")]
    UrlRefused,
    /// Stopped through a `CancelToken`; partial data is discarded, nothing is written.
    #[error("annulé")]
    Cancelled,
    /// `CoverChain`: no source could be asked (none configured, or all refused their credentials).
    #[error("aucune source de jaquettes disponible")]
    NoSource,
    /// A cover source answered only 404 (no success) many times in a row: judged down for the session. The
    /// result is inconclusive (nothing must be recorded as « absent »); no URL.
    #[error("source de jaquettes en panne")]
    SourceBroken,
    #[error("{path} : {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl AssetError {
    /// Worth retrying after a delay (ADR 0013: network errors, timeouts, 5xx). Never `Cancelled`, size limits,
    /// refused URLs or 4xx.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::Http { status } => (500..600).contains(status),
            _ => false,
        }
    }
}
