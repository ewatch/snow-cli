use crate::auth::Authenticator;
use crate::config::{
    AuthMethod,
    native_session::{self, NativeSession},
};
use async_trait::async_trait;
use http::HeaderMap;

/// Native cookie authentication never falls back to Basic/OAuth or an environment cookie.
#[derive(Debug)]
pub struct NativeSessionAuth {
    session: NativeSession,
    profile_name: Option<String>,
}
impl NativeSessionAuth {
    pub fn stored(profile: &str, instance: &str) -> anyhow::Result<Self> {
        let session = native_session::load(profile).map_err(|error| {
            error.context(crate::auth::AuthenticationError::new(
                "No valid native session is available; use session login or session attach.",
            ))
        })?;
        session.ensure_instance(instance).map_err(|error| error.context(
            crate::auth::AuthenticationError::new("Native session belongs to a different instance; refusing to send session credentials")
        ))?;
        Ok(Self {
            session,
            profile_name: Some(profile.to_string()),
        })
    }
    pub fn transient(session: NativeSession) -> Self {
        Self {
            session,
            profile_name: None,
        }
    }
}
#[async_trait]
impl Authenticator for NativeSessionAuth {
    async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
        self.session.headers()
    }
    async fn refresh(&mut self) -> anyhow::Result<bool> {
        Ok(false)
    }
    fn native_session_credentials(&self) -> Option<NativeSession> {
        Some(self.session.clone())
    }
    async fn update_native_csrf(&mut self, token: &str) -> anyhow::Result<()> {
        if self.session.csrf_token.expose_secret() == token {
            return Ok(());
        }
        self.session.csrf_token = crate::models::secret::Secret::new(token.to_string());
        if let Some(profile) = &self.profile_name {
            native_session::save(profile, &self.session)?;
        }
        Ok(())
    }
    fn auth_type(&self) -> AuthMethod {
        AuthMethod::NativeSession
    }
    async fn observe_response_headers(&mut self, headers: &HeaderMap) -> anyhow::Result<()> {
        if self.session.merge_response_cookies(headers)
            && let Some(profile) = &self.profile_name
        {
            native_session::save(profile, &self.session)?;
        }
        Ok(())
    }
}
