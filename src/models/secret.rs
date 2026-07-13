//! A wrapper that keeps a sensitive value out of `Debug`, `Display`, and logs
//! *by construction* rather than by convention.
//!
//! The guidelines require that secrets (tokens, passwords, client secrets)
//! never leak through `Debug` or `tracing`. Hand-writing a redacting `Debug`
//! for every struct that holds a secret is easy to forget — a new field or a
//! new struct silently reintroduces the leak. Wrapping the value in
//! [`Secret`] instead makes the guarantee part of the type: a `#[derive(Debug)]`
//! on the containing struct is now safe, and the only way to read the value is
//! the explicit, greppable [`Secret::expose_secret`] call.
//!
//! `serde` support is intentionally *transparent*: serializing a `Secret<T>`
//! writes the real value (so tokens can still be persisted to the keychain),
//! while `Debug`/`Display` never reveal it. Redaction is a formatting concern,
//! not a serialization one.

use std::fmt;

/// Placeholder rendered anywhere a secret would otherwise be printed.
const REDACTED: &str = "<redacted>";

/// Wraps a sensitive value so it cannot accidentally leak through `Debug`,
/// `Display`, or `tracing`. Read the inner value only through
/// [`Secret::expose_secret`].
///
/// Defaults to wrapping a `String`, the common case (API tokens, passwords).
#[derive(Clone, PartialEq, Eq)]
pub struct Secret<T = String>(T);

impl<T> Secret<T> {
    /// Wrap a value, marking it as secret.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrow the underlying secret. This is the single, deliberately explicit
    /// escape hatch — grep for `expose_secret` to audit every place a secret
    /// is actually read.
    pub fn expose_secret(&self) -> &T {
        &self.0
    }

    /// Consume the wrapper and return the owned secret.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Keep the `Secret(...)` shape so `#[derive(Debug)]` on a containing
        // struct reads naturally, but never show the value.
        write!(f, "Secret({REDACTED})")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<T> From<T> for Secret<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

/// Transparent serialization: the real value is written, so secrets round-trip
/// through persisted config/keychain payloads unchanged.
impl<T: serde::Serialize> serde::Serialize for Secret<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Transparent deserialization: a plain scalar in JSON becomes a `Secret<T>`.
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Secret<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(T::deserialize(deserializer)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_the_value() {
        let secret = Secret::new("super-secret-token".to_string());
        let rendered = format!("{secret:?}");
        assert_eq!(rendered, "Secret(<redacted>)");
        assert!(!rendered.contains("super-secret-token"));
    }

    #[test]
    fn display_never_reveals_the_value() {
        let secret = Secret::new("super-secret-token".to_string());
        assert_eq!(format!("{secret}"), "<redacted>");
    }

    #[test]
    fn debug_of_containing_struct_is_safe() {
        // The whole point: a derived Debug on a struct with a secret field
        // stays redacted without any hand-written impl.
        #[derive(Debug)]
        #[allow(dead_code)] // fields are exercised through the derived Debug only
        struct Token {
            access_token: Secret<String>,
            refresh_token: Option<Secret<String>>,
            expires_in: u64,
        }

        let token = Token {
            access_token: Secret::new("access-xyz".to_string()),
            refresh_token: Some(Secret::new("refresh-xyz".to_string())),
            expires_in: 3600,
        };
        let rendered = format!("{token:?}");
        assert!(!rendered.contains("access-xyz"));
        assert!(!rendered.contains("refresh-xyz"));
        // Non-secret structure is still visible for debugging.
        assert!(rendered.contains("expires_in: 3600"));
        // `Some`/`None` distinction survives without leaking the value.
        assert!(rendered.contains("Some(Secret(<redacted>))"));
    }

    #[test]
    fn expose_secret_returns_the_real_value() {
        let secret = Secret::new("real".to_string());
        assert_eq!(secret.expose_secret(), "real");
        assert_eq!(secret.into_inner(), "real");
    }

    #[test]
    fn serde_is_transparent_for_persistence() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Payload {
            access_token: Secret<String>,
        }

        let json = serde_json::to_string(&Payload {
            access_token: Secret::new("persist-me".to_string()),
        })
        .unwrap();
        // The real value is written (needed for keychain round-trips)...
        assert_eq!(json, r#"{"access_token":"persist-me"}"#);

        // ...and reads back into a Secret.
        let parsed: Payload = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.access_token.expose_secret(), "persist-me");
    }

    #[test]
    fn from_and_equality() {
        let a: Secret<String> = "x".to_string().into();
        let b = Secret::new("x".to_string());
        assert_eq!(a, b);
    }
}
