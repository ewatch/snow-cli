//! Native UI endpoints, observed on dev404396. These are release-dependent UI
//! contracts, not documented public REST APIs; unsupported responses fail explicitly.
use super::SnowClient;
use crate::config::native_session::NativeSession;
use crate::models::{
    secret::Secret,
    session::{SessionInspection, SessionSelection, SessionSwitchType, SessionUser},
};
use http::Method;
use serde_json::{Value, json};

const BOOTSTRAP: &str = "/navpage.do";
const PICKERS: &str = "/api/now/ui/concoursepicker/current";

impl SnowClient {
    pub async fn native_login(
        &mut self,
        username: &str,
        password: &Secret,
    ) -> anyhow::Result<NativeSession> {
        self.authorize_instance_request(&Method::POST, "/login.do")?;
        let cookie_header = self
            .form_login_cookie_header(username, password.expose_secret())
            .await?;
        let mut session = NativeSession {
            instance: self.base_url().to_string(),
            cookie_header: Secret::new(cookie_header),
            csrf_token: Secret::new(String::new()),
            authenticated_user: Some(username.to_string()),
        };
        // Verify cookie-only authentication, not a Basic request that could mask a failed login.
        let mut client = self.transient_native_client(session.clone())?;
        client.refresh_native_csrf().await?;
        let user = client.native_current_user().await?;
        anyhow::ensure!(
            user.user_name == username,
            "Native login authenticated an unexpected user"
        );
        session = client.native_credentials().await?;
        session.authenticated_user = Some(username.to_string());
        Ok(session)
    }

    pub fn transient_native_client(&self, session: NativeSession) -> anyhow::Result<SnowClient> {
        session.ensure_instance(self.base_url())?;
        SnowClient::with_config(
            self.base_url().to_string(),
            Box::new(crate::auth::native_session::NativeSessionAuth::transient(
                session,
            )),
            super::ClientConfig {
                timeout_secs: self.timeout_secs,
                policy: self.policy,
            },
        )
    }

    pub async fn native_credentials(&self) -> anyhow::Result<NativeSession> {
        self.authenticator
            .native_session_credentials()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "This command requires a native_session profile; use session login/attach first"
                )
            })
    }

    async fn refresh_native_csrf(&mut self) -> anyhow::Result<Value> {
        self.native_credentials().await?;
        let response = self.native_bootstrap_response().await?;
        let headers = response.headers().clone();
        anyhow::ensure!(
            !matches!(
                super::session::logged_in_header_value(&headers),
                Some(false)
            ),
            "Native session has expired or is not authenticated; log in or attach again"
        );
        let body = response.text().await?;
        let token = super::extract_g_ck_from_body(&body)
            .ok_or_else(|| anyhow::anyhow!("Native session bootstrap did not return a CSRF token; session may be expired or this UI is unsupported"))?;
        self.authenticator.update_native_csrf(&token).await?;
        let bootstrap = super::session_bootstrap::parse_bootstrap(&body).unwrap_or(Value::Null);
        anyhow::ensure!(
            bootstrap
                .pointer("/output/isLoggedIn")
                .and_then(Value::as_bool)
                != Some(false),
            "Native session is not logged in; log in or attach again"
        );
        Ok(bootstrap)
    }

    async fn native_bootstrap_response(&mut self) -> anyhow::Result<super::ClientResponse> {
        let mut url = self.authorize_instance_request(&Method::GET, BOOTSTRAP)?;
        // Native shells redirect navpage.do to their home route. Follow these GETs
        // explicitly, with origin checks and rotated cookies at every hop.
        for _ in 0..5 {
            let request = self
                .http
                .get(&url)
                .header("Accept", "text/html,application/xhtml+xml")
                .headers(self.authenticator.authenticate().await?)
                .build()?;
            super::log_raw_http_request(&request);
            let response = self.http.execute(request).await?;
            super::log_raw_http_response(&url, response.status(), response.headers());
            self.authenticator
                .observe_response_headers(response.headers())
                .await?;
            anyhow::ensure!(
                super::session::logged_in_header_value(response.headers()) != Some(false),
                "Native session has expired or is not authenticated; log in or attach again"
            );
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(http::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| anyhow::anyhow!("Native bootstrap redirect has no Location"))?;
                let next = url::Url::parse(&url)?.join(location)?;
                anyhow::ensure!(
                    next.path() != "/login.do",
                    "Native session redirected to login; log in or attach again"
                );
                url = self.authorize_instance_request(&Method::GET, next.as_str())?;
                continue;
            }
            anyhow::ensure!(
                response.status().is_success(),
                "Native session bootstrap failed (HTTP {})",
                response.status().as_u16()
            );
            return Ok(super::ClientResponse { inner: response });
        }
        anyhow::bail!("Native session bootstrap exceeded the redirect limit")
    }

    async fn native_current_user(&mut self) -> anyhow::Result<SessionUser> {
        let response: Value = self.get_json("/api/now/ui/user/current_user").await?;
        let user = response
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("Unsupported current-user response"))?;
        let sys_id = text(user, "user_sys_id")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Native session has no authenticated user"))?;
        let user_name = text(user, "user_name")
            .filter(|value| value != "guest" && !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Native session is unauthenticated or expired"))?;
        Ok(SessionUser {
            sys_id,
            user_name,
            display_name: text(user, "user_display_name"),
        })
    }

    pub async fn inspect_native_session(&mut self) -> anyhow::Result<SessionInspection> {
        let bootstrap = self.refresh_native_csrf().await?;
        let credentials = self.native_credentials().await?;
        let effective_user = self.native_current_user().await?;
        let response: Value = self.get_json(PICKERS).await?;
        let context = response
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("Unsupported native picker response"))?;
        let metadata = bootstrap.pointer("/output/user").unwrap_or(&Value::Null);
        if let Some(bootstrap_id) = text(metadata, "sys_id") {
            anyhow::ensure!(
                bootstrap_id == effective_user.sys_id,
                "Session identity changed during inspection; run session show again"
            );
        }
        let roles = string_array(metadata.get("roles"));
        let available_elevated_roles = string_array(metadata.get("elevatedRoles"));
        let active_elevated_roles =
            roles
                .as_ref()
                .zip(available_elevated_roles.as_ref())
                .map(|(roles, available)| {
                    available
                        .iter()
                        .filter(|role| roles.contains(role))
                        .cloned()
                        .collect()
                });
        let mut unavailable = Vec::new();
        if metadata.is_null() {
            unavailable.push("bootstrap_user_metadata");
        }
        let domain = selection(context.get("currentDomain"), "value");
        let application = selection(context.get("currentApplication"), "sysId");
        let update_set = selection(context.get("currentUpdateSet"), "sysId");
        if domain.is_none() {
            unavailable.push("domain");
        }
        if application.is_none() {
            unavailable.push("application");
        }
        if update_set.is_none() {
            unavailable.push("update_set");
        }
        // A session-domain value is not proof of home domain. Read the user record separately.
        let sys_id: crate::models::identifiers::SysId = effective_user.sys_id.parse()?;
        let profile = self
            .get_json::<Value>(&format!(
                "/api/now/table/sys_user/{sys_id}?sysparm_fields=sys_domain"
            ))
            .await;
        let home_domain = match profile {
            Ok(value) => value
                .pointer("/result/sys_domain/value")
                .or_else(|| value.pointer("/result/sys_domain"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            Err(error)
                if error
                    .downcast_ref::<super::error::ApiError>()
                    .is_some_and(|error| matches!(error.status, 403 | 404)) =>
            {
                unavailable.push("home_domain");
                None
            }
            Err(error) => return Err(error),
        };
        if home_domain.is_none() && !unavailable.contains(&"home_domain") {
            unavailable.push("home_domain");
        }
        Ok(SessionInspection {
            instance: self.base_url().to_string(),
            transport: "native_cookie",
            valid: true,
            authenticated_user: credentials.authenticated_user,
            effective_user,
            impersonating: metadata.get("isImpersonating").and_then(Value::as_bool),
            domain,
            application,
            update_set,
            home_domain,
            record_domain: None,
            roles,
            available_elevated_roles,
            active_elevated_roles,
            settings: whitelist(
                metadata,
                &[
                    "language",
                    "timeZone",
                    "dateFormat",
                    "timeFormat",
                    "dateTimeFormat",
                    "locale",
                    "currency",
                    "encryptionContexts",
                    "decimalSeparator",
                    "groupingSeparator",
                ],
            ),
            session_properties: whitelist(
                bootstrap
                    .pointer("/output/properties")
                    .unwrap_or(&Value::Null),
                &["sessionTimeout", "sessionTimeleft", "activeSessionTimeleft"],
            ),
            unavailable,
        })
    }

    pub async fn native_session_options(
        &mut self,
        kind: SessionSwitchType,
    ) -> anyhow::Result<Vec<SessionSelection>> {
        self.refresh_native_csrf().await?;
        let response: Value = self
            .get_json(&format!("/api/now/ui/concoursepicker/{}", kind.endpoint()))
            .await?;
        let result = response
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("Unsupported native picker-options response"))?;
        let (key, id) = match kind {
            SessionSwitchType::Updateset => ("updateSet", "sysId"),
            SessionSwitchType::Domain => ("list", "value"),
            SessionSwitchType::Application => ("list", "sysId"),
        };
        let options = result
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("Native picker did not return selectable options"))?;
        Ok(options
            .iter()
            .filter_map(|value| selection(Some(value), id))
            .collect())
    }

    pub async fn switch_native_session(
        &mut self,
        kind: SessionSwitchType,
        value: &str,
    ) -> anyhow::Result<(SessionInspection, SessionInspection)> {
        self.authorize_instance_request(&Method::PUT, "/api/now/ui/concoursepicker/current")?;
        let before = self.inspect_native_session().await?;
        let options = self.native_session_options(kind).await?;
        // Resolve application scope names only through the actual accessible picker.
        let candidates = options
            .iter()
            .filter(|option| {
                option.id == value
                    || (matches!(kind, SessionSwitchType::Application)
                        && option.scope.as_deref() == Some(value))
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(
            candidates.len() == 1,
            "Requested context is unavailable or ambiguous in this session's picker; use session options"
        );
        let selected = candidates[0];
        let current = before.selection(kind);
        // Some UI versions reject selecting their already-current Default update set.
        // The fresh inspection and accessible picker already prove the desired context.
        if current.is_some_and(|current| current.id == selected.id) {
            return Ok((before.clone(), before));
        }
        let body = match kind {
            SessionSwitchType::Domain => json!({"value":selected.id}),
            SessionSwitchType::Application => json!({"app_id":selected.id}),
            SessionSwitchType::Updateset => json!({"sysId":selected.id,"name":selected.name}),
        };
        let path = format!("/api/now/ui/concoursepicker/{}", kind.endpoint());
        self.put(&path, &body.to_string()).await.map_err(|error| {
            error.context("Native context switch failed. Session may have changed; run session show. No automatic retry or rollback was attempted.")
        })?;
        let after = self.inspect_native_session().await.map_err(|_| anyhow::anyhow!("Context switch was sent, but read-back failed. Session may have changed; run session show. No automatic retry or rollback was attempted."))?;
        let actual = after.selection(kind);
        anyhow::ensure!(
            actual.is_some_and(|actual| actual.id == selected.id),
            "Context switch was sent but read-back did not match the requested value. Session may have changed; run session show. No automatic retry or rollback was attempted."
        );
        Ok((before, after))
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}
fn string_array(value: Option<&Value>) -> Option<Vec<String>> {
    let values = value?.as_array()?;
    values
        .iter()
        .map(|value| value.as_str().map(ToOwned::to_owned))
        .collect()
}
fn selection(value: Option<&Value>, id: &str) -> Option<SessionSelection> {
    let value = value?;
    Some(SessionSelection {
        id: text(value, id).filter(|id| !id.is_empty())?,
        name: text(value, "name").or_else(|| text(value, "label")),
        scope: text(value, "scopeName"),
    })
}
fn whitelist(value: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .map(|key| {
                (
                    (*key).to_string(),
                    value.get(*key).cloned().unwrap_or(Value::Null),
                )
            })
            .collect(),
    )
}
