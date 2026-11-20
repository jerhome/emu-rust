use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// A credential (ScreenScraper password, IGDB client secret…). `Debug` is redacted and there is no `Display`, so it
/// cannot reach a log or a message by accident; `expose` is the only way to read it. The buffer is wiped on drop
/// (copies made by callers of `expose` are theirs to handle). Equality does not stop at the first differing byte.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        let (a, b) = (self.0.as_bytes(), other.0.as_bytes());
        // Only the length may leak; the contents are compared in full.
        if a.len() != b.len() {
            return false;
        }
        let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
        std::hint::black_box(diff) == 0
    }
}

impl Eq for Secret {}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    #[test]
    fn debug_is_redacted() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s:?} {s:#?}"), "Secret(***) Secret(***)");
    }

    #[test]
    fn equality() {
        assert_eq!(Secret::new("abc"), Secret::new("abc"));
        assert_ne!(Secret::new("abc"), Secret::new("abd"));
        assert_ne!(Secret::new("abc"), Secret::new("abcd"));
        assert_eq!(Secret::default(), Secret::new(""));
        assert!(Secret::default().is_empty());
    }

    #[test]
    fn serde_is_transparent() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct C {
            password: Secret,
        }
        let c: C = toml::from_str("password = \"hunter2\"").unwrap();
        assert_eq!(c.password.expose(), "hunter2");
        assert_eq!(
            toml::to_string(&c).unwrap().trim(),
            "password = \"hunter2\""
        );
    }
}
