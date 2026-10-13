//! Script responses are buffered and stripped of credentials used by their
//! transport before they cross the client seam. Unrelated script output remains
//! lossless; this is not a general-purpose scrubber for arbitrary user data.
use crate::models::secret::Secret;
use http::HeaderMap;
use std::fmt;

/// Owned script result with no transport object or authentication headers.
pub struct BackgroundScriptResponse {
    pub(super) status: u16,
    pub(super) final_url: String,
    pub(super) server_timing: Option<String>,
    pub(super) body: String,
}

impl BackgroundScriptResponse {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn final_url(&self) -> &str {
        &self.final_url
    }

    pub fn server_timing(&self) -> Option<&str> {
        self.server_timing.as_deref()
    }

    pub async fn text(self) -> anyhow::Result<String> {
        Ok(self.body)
    }
}

impl fmt::Debug for BackgroundScriptResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackgroundScriptResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
pub(super) struct ScriptRedactions(Vec<Secret>);

impl ScriptRedactions {
    fn add(&mut self, value: &str) {
        if !value.is_empty() {
            self.0.push(Secret::new(value.to_string()));
            self.0
                .push(Secret::new(urlencoding::encode(value).into_owned()));
        }
    }

    pub(super) fn observe_request(&mut self, headers: &HeaderMap) {
        if let Some(value) = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        {
            self.add(value);
            if let Some((scheme, credential)) = value.split_once(' ') {
                self.add(credential);
                if scheme.eq_ignore_ascii_case("basic") {
                    use base64::Engine;
                    if let Ok(decoded) =
                        base64::engine::general_purpose::STANDARD.decode(credential)
                        && let Ok(decoded) = String::from_utf8(decoded)
                        && let Some((_, password)) = decoded.split_once(':')
                    {
                        self.add(password);
                    }
                }
            }
        }
        if let Some(value) = headers
            .get(http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
        {
            self.add(value);
            for cookie in value.split(';') {
                if let Some((_, value)) = cookie.trim().split_once('=') {
                    self.add(value);
                }
            }
        }
        if let Some(value) = headers.get("X-UserToken").and_then(|v| v.to_str().ok()) {
            self.add(value);
        }
    }

    pub(super) fn observe_response(&mut self, headers: &HeaderMap) {
        for cookie in headers.get_all(http::header::SET_COOKIE) {
            if let Ok(cookie) = cookie.to_str()
                && let Some((_, value)) = cookie.split(';').next().and_then(|v| v.split_once('='))
            {
                self.add(value);
            }
        }
    }

    pub(super) fn redact(mut self, mut body: String) -> String {
        // Replace longest values first so a short cookie cannot leave a suffix of
        // another credential visible. Preserve whitespace and execution-log framing.
        self.0
            .sort_by_key(|secret| std::cmp::Reverse(secret.expose_secret().len()));
        self.0.dedup();
        for secret in self.0 {
            body = body.replace(secret.expose_secret(), "<redacted>");
        }
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_debug_does_not_log_output_or_headers() {
        let response = BackgroundScriptResponse {
            status: 200,
            final_url: "https://private.example/?token=private".into(),
            server_timing: Some("private".into()),
            body: "private script output".into(),
        };
        assert!(!format!("{response:?}").contains("private"));
    }

    #[test]
    fn redaction_preserves_framing_and_covers_encoded_and_rotated_values() {
        let mut redactions = ScriptRedactions::default();
        let mut headers = HeaderMap::new();
        headers.insert("Cookie", "JSESSIONID=session+private".parse().unwrap());
        headers.insert("X-UserToken", "csrf-private".parse().unwrap());
        redactions.observe_request(&headers);
        headers.insert(
            "Set-Cookie",
            "JSESSIONID=rotated-private; Path=/".parse().unwrap(),
        );
        redactions.observe_response(&headers);
        assert_eq!(
            redactions.redact("session%2Bprivate\ncsrf-private\nrotated-private\nkeep this".into()),
            "<redacted>\n<redacted>\n<redacted>\nkeep this"
        );
    }
}
