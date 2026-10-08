use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUser {
    pub sys_id: String,
    pub user_name: String,
    pub display_name: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSelection {
    pub id: String,
    pub name: Option<String>,
    pub scope: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct SessionInspection {
    pub instance: String,
    pub transport: &'static str,
    pub valid: bool,
    pub authenticated_user: Option<String>,
    pub effective_user: SessionUser,
    pub impersonating: Option<bool>,
    pub domain: Option<SessionSelection>,
    pub application: Option<SessionSelection>,
    pub update_set: Option<SessionSelection>,
    pub home_domain: Option<String>,
    pub record_domain: Option<String>,
    pub roles: Option<Vec<String>>,
    pub available_elevated_roles: Option<Vec<String>>,
    pub active_elevated_roles: Option<Vec<String>>,
    pub settings: serde_json::Value,
    pub session_properties: serde_json::Value,
    pub unavailable: Vec<&'static str>,
}
impl SessionInspection {
    pub fn selection(&self, kind: SessionSwitchType) -> Option<&SessionSelection> {
        match kind {
            SessionSwitchType::Domain => self.domain.as_ref(),
            SessionSwitchType::Application => self.application.as_ref(),
            SessionSwitchType::Updateset => self.update_set.as_ref(),
        }
    }
}
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SessionSwitchType {
    Domain,
    Application,
    Updateset,
}
impl SessionSwitchType {
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::Application => "application",
            Self::Updateset => "updateset",
        }
    }
}
