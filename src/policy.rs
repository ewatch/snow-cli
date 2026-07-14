use std::sync::atomic::{AtomicU8, Ordering};

use http::Method;
use thiserror::Error;

use crate::cli::args::{
    ApiCommands, AttachmentCommands, AuthCommands, CodesearchCommands, Commands, ConfigCommands,
    DataCommands, ImportSetCommands, ProfileSdkCommands, ScopeCommands, ScriptCommands,
    SeedCommands, SkillCommands, SnuCommands, SnuContextCommands, TableCommands,
};

static ACTIVE_POLICY_MODE: AtomicU8 = AtomicU8::new(PolicyMode::FullAccess as u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PolicyMode {
    FullAccess = 0,
    ReadOnly = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionPolicy {
    mode: PolicyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCapability {
    RemoteRead,
    RemoteWrite,
    RawApiRead,
    RawApiWrite,
    CredentialExport,
    LocalConfigRead,
    LocalConfigWrite,
    LocalCredentialRead,
    LocalCredentialWrite,
    LocalFileWrite,
    Unknown,
}

impl CommandCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RemoteRead => "remote_read",
            Self::RemoteWrite => "remote_write",
            Self::RawApiRead => "raw_api_read",
            Self::RawApiWrite => "raw_api_write",
            Self::CredentialExport => "credential_export",
            Self::LocalConfigRead => "local_config_read",
            Self::LocalConfigWrite => "local_config_write",
            Self::LocalCredentialRead => "local_credential_read",
            Self::LocalCredentialWrite => "local_credential_write",
            Self::LocalFileWrite => "local_file_write",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny {
        command: &'static str,
        capability: CommandCapability,
        reason: &'static str,
    },
}

#[derive(Debug, Error)]
#[error("Policy denied {operation}: {reason}")]
pub struct PolicyError {
    pub operation: String,
    pub mode: PolicyMode,
    pub capability: CommandCapability,
    pub reason: String,
}

impl PolicyError {
    pub fn code(&self) -> &'static str {
        "POLICY_DENIED"
    }
}

impl ExecutionPolicy {
    pub const fn full_access() -> Self {
        Self {
            mode: PolicyMode::FullAccess,
        }
    }

    pub const fn read_only() -> Self {
        Self {
            mode: PolicyMode::ReadOnly,
        }
    }

    pub const fn mode(self) -> PolicyMode {
        self.mode
    }

    pub fn ensure_command_allowed(&self, command: &Commands) -> Result<(), PolicyError> {
        match self.decision_for_command(command) {
            PolicyDecision::Allow => Ok(()),
            PolicyDecision::Deny {
                command,
                capability,
                reason,
            } => Err(PolicyError {
                operation: command.to_string(),
                mode: self.mode,
                capability,
                reason: reason.to_string(),
            }),
        }
    }

    pub fn ensure_request_allowed(&self, method: &Method, path: &str) -> Result<(), PolicyError> {
        if self.mode == PolicyMode::FullAccess || method == Method::GET {
            // `api get` and other GET requests are allowed in read-only mode by
            // normal HTTP/API semantics. A custom endpoint that mutates on GET is
            // considered a bad API design; pair read-only mode with read-only
            // ServiceNow credentials for stronger guarantees.
            return Ok(());
        }

        Err(PolicyError {
            operation: format!("HTTP {method} {path}"),
            mode: self.mode,
            capability: CommandCapability::RemoteWrite,
            reason: "read-only policy permits HTTP GET requests only".to_string(),
        })
    }

    pub fn ensure_raw_api_headers_allowed(
        &self,
        method: &Method,
        extra_headers: &[(String, String)],
    ) -> Result<(), PolicyError> {
        if self.mode != PolicyMode::ReadOnly || method != Method::GET {
            return Ok(());
        }

        let has_method_override = extra_headers.iter().any(|(name, _)| {
            matches!(
                name.trim().to_ascii_lowercase().as_str(),
                "x-http-method-override" | "x-method-override" | "x-http-method"
            )
        });

        if has_method_override {
            return Err(PolicyError {
                operation: "api get".to_string(),
                mode: PolicyMode::ReadOnly,
                capability: CommandCapability::RawApiWrite,
                reason: "read-only policy does not allow method override headers on api get"
                    .to_string(),
            });
        }

        Ok(())
    }

    pub fn decision_for_command(&self, command: &Commands) -> PolicyDecision {
        if self.mode == PolicyMode::FullAccess {
            return PolicyDecision::Allow;
        }

        read_only_command_decision(command)
    }
}

pub fn set_active_policy(policy: ExecutionPolicy) {
    ACTIVE_POLICY_MODE.store(policy.mode as u8, Ordering::SeqCst);
}

pub fn active_policy() -> ExecutionPolicy {
    match ACTIVE_POLICY_MODE.load(Ordering::SeqCst) {
        1 => ExecutionPolicy::read_only(),
        _ => ExecutionPolicy::full_access(),
    }
}

fn read_only_command_decision(command: &Commands) -> PolicyDecision {
    match command {
        // Profile management writes local configuration (`~/.servicenow/config.toml`)
        // and never mutates the remote ServiceNow instance, so it is permitted in
        // read-only mode. This lets snow-cli-ro be used standalone to add, edit, and
        // select the profiles it reads from.
        //
        // The now-sdk *export* commands are the exception: they read a stored
        // password from the OS keychain and write it, in plaintext, into the
        // now-sdk alias store on disk. That materializes a reusable, write-capable
        // credential a full client could replay — the same risk that keeps
        // `auth token` denied — so export stays blocked. Import is the inbound
        // direction (now-sdk store -> keychain) and remains allowed.
        Commands::Profile(args) => match &args.command {
            ConfigCommands::ExportNowSdk { .. } => deny(
                "profile export-now-sdk",
                CommandCapability::CredentialExport,
                "read-only policy does not allow exporting reusable credentials to the now-sdk store",
            ),
            ConfigCommands::Sdk(sdk_args) => match &sdk_args.command {
                ProfileSdkCommands::Export { .. } => deny(
                    "profile sdk export",
                    CommandCapability::CredentialExport,
                    "read-only policy does not allow exporting reusable credentials to the now-sdk store",
                ),
                ProfileSdkCommands::List | ProfileSdkCommands::Import { .. } => {
                    PolicyDecision::Allow
                }
            },
            _ => PolicyDecision::Allow,
        },
        Commands::Auth(args) => match &args.command {
            // login/logout only write local credentials (OS keychain / config) and do
            // not grant any additional remote access beyond what the credentials' own
            // ServiceNow ACLs allow. They are permitted so snow-cli-ro can bootstrap
            // authentication on its own.
            AuthCommands::Status | AuthCommands::Login { .. } | AuthCommands::Logout => {
                PolicyDecision::Allow
            }
            // `auth token` exports a reusable credential/bearer token that could be
            // replayed by a full client to perform writes, so it stays denied.
            AuthCommands::Token => deny(
                "auth token",
                CommandCapability::CredentialExport,
                "read-only policy does not allow exporting reusable credentials",
            ),
        },
        Commands::Table(args) => match &args.command {
            TableCommands::List { .. }
            | TableCommands::Get { .. }
            | TableCommands::Schema { .. }
            | TableCommands::Stats { .. } => PolicyDecision::Allow,
            TableCommands::Create { .. }
            | TableCommands::Update { .. }
            | TableCommands::Delete { .. } => deny(
                "table write",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow table mutations",
            ),
        },
        Commands::Data(args) => match &args.command {
            DataCommands::Export { .. }
            | DataCommands::ExportPackage { .. }
            | DataCommands::Validate { .. } => PolicyDecision::Allow,
            DataCommands::Import { .. } => deny(
                "data import",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow data import",
            ),
        },
        Commands::Seed(args) => match &args.command {
            SeedCommands::Plan { .. } => PolicyDecision::Allow,
            SeedCommands::Apply { .. } | SeedCommands::Cleanup { .. } => deny(
                "seed apply/cleanup",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow seed mutations or cleanup deletes",
            ),
        },
        Commands::Scope(args) => match &args.command {
            ScopeCommands::List { .. }
            | ScopeCommands::Inspect { .. }
            | ScopeCommands::Inventory { .. } => PolicyDecision::Allow,
            ScopeCommands::MoveFile { .. } => deny(
                "scope move-file",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow moving application files between scopes",
            ),
        },
        Commands::Attachment(args) => match &args.command {
            AttachmentCommands::List { .. } | AttachmentCommands::Download { .. } => {
                PolicyDecision::Allow
            }
            AttachmentCommands::Upload { .. } => deny(
                "attachment upload",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow attachment uploads",
            ),
        },
        Commands::ImportSet(args) => match &args.command {
            ImportSetCommands::Load { .. } | ImportSetCommands::Transform { .. } => deny(
                "import-set",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow import set operations",
            ),
        },
        Commands::Api(args) => match &args.command {
            ApiCommands::Get { .. } => PolicyDecision::Allow,
            ApiCommands::Post { .. } | ApiCommands::Put { .. } | ApiCommands::Delete { .. } => {
                deny(
                    "api write",
                    CommandCapability::RawApiWrite,
                    "read-only policy allows raw API GET only",
                )
            }
        },
        Commands::Script(args) => match &args.command {
            ScriptCommands::Run { .. } => deny(
                "script run",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow background script execution",
            ),
        },
        Commands::Codesearch(args) => match &args.command {
            CodesearchCommands::Search { .. } => PolicyDecision::Allow,
        },
        Commands::Snu(args) => match &args.command {
            SnuCommands::CheckConnection { .. }
            | SnuCommands::GetInstanceInfo { .. }
            | SnuCommands::WaitToken { .. }
            | SnuCommands::ListTables { .. }
            | SnuCommands::GetRecord { .. }
            | SnuCommands::AppMeta { .. }
            | SnuCommands::Query { .. }
            | SnuCommands::Schema { .. }
            | SnuCommands::Slash { .. }
            | SnuCommands::Tab(_)
            | SnuCommands::Screenshot { .. }
            | SnuCommands::Broker(_) => PolicyDecision::Allow,
            SnuCommands::UpdateRecord { .. } => deny(
                "snu update-record",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow record updates through SN-Utils",
            ),
            SnuCommands::CreateRecord { .. } => deny(
                "snu create-record",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow record creation through SN-Utils",
            ),
            SnuCommands::DeleteRecord { .. } => deny(
                "snu delete-record",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow record deletion through SN-Utils",
            ),
            SnuCommands::ExecuteBgScript { .. } => deny(
                "snu execute-bg-script",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow background script execution through SN-Utils",
            ),
            SnuCommands::Context(context_args) => match &context_args.command {
                SnuContextCommands::Switch { .. } => deny(
                    "snu context switch",
                    CommandCapability::RemoteWrite,
                    "read-only policy does not allow switching browser session context",
                ),
            },
            SnuCommands::AttachmentUpload { .. } => deny(
                "snu attachment upload",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow attachment uploads",
            ),
        },
        Commands::Skill(args) => match &args.command {
            SkillCommands::Install { .. } => PolicyDecision::Allow,
        },
        Commands::Completions { .. } => PolicyDecision::Allow,
    }
}

fn deny(
    command: &'static str,
    capability: CommandCapability,
    reason: &'static str,
) -> PolicyDecision {
    PolicyDecision::Deny {
        command,
        capability,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::*;
    use clap_complete::Shell;

    fn policy() -> ExecutionPolicy {
        ExecutionPolicy::read_only()
    }

    fn assert_allowed(command: Commands) {
        assert_eq!(
            policy().decision_for_command(&command),
            PolicyDecision::Allow
        );
    }

    fn assert_denied(command: Commands) {
        assert!(matches!(
            policy().decision_for_command(&command),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn read_only_allows_expected_commands() {
        assert_allowed(Commands::Profile(ConfigArgs {
            command: ConfigCommands::ListProfiles,
        }));
        assert_allowed(Commands::Profile(ConfigArgs {
            command: ConfigCommands::Sdk(ProfileSdkArgs {
                command: ProfileSdkCommands::List,
            }),
        }));
        assert_allowed(Commands::Auth(AuthArgs {
            command: AuthCommands::Status,
        }));
        assert_allowed(Commands::Table(TableArgs {
            command: TableCommands::List {
                table: "incident".parse().unwrap(),
                query: None,
                fields: None,
                limit: Some(1),
                all: false,
                order_by: None,
                full: false,
            },
        }));
        assert_allowed(Commands::Table(TableArgs {
            command: TableCommands::Get {
                table: "incident".parse().unwrap(),
                sys_id: "6816f79cc0a8016401c5a33be04be441".parse().unwrap(),
                fields: None,
                full: false,
            },
        }));
        assert_allowed(Commands::Table(TableArgs {
            command: TableCommands::Schema {
                table: "incident".parse().unwrap(),
                extended: false,
                include_inherited: false,
            },
        }));
        assert_allowed(Commands::Table(TableArgs {
            command: TableCommands::Stats {
                table: "incident".parse().unwrap(),
                query: None,
                group_by: Some("state".to_string()),
                avg: None,
                min: None,
                max: None,
                sum: None,
                having: None,
            },
        }));
        assert_allowed(Commands::Api(ApiArgs {
            command: ApiCommands::Get {
                path: "/api/now/table/incident".to_string(),
                header: vec![],
            },
        }));
        assert_allowed(Commands::Codesearch(CodesearchArgs {
            command: CodesearchCommands::Search {
                query: "foo".to_string(),
                source_table: None,
                scope: None,
                limit: 10,
                current_scope: false,
                search_group: "sn_devstudio.Studio Search Group".to_string(),
            },
        }));
        assert_allowed(Commands::Completions { shell: Shell::Bash });
    }

    #[test]
    fn read_only_allows_snu_read_commands() {
        assert_allowed(Commands::Snu(SnuArgs {
            command: SnuCommands::CheckConnection {
                timeout_secs: 1,
                verify: false,
            },
        }));
        assert_allowed(Commands::Snu(SnuArgs {
            command: SnuCommands::GetInstanceInfo { timeout_secs: 1 },
        }));
        assert_allowed(Commands::Snu(SnuArgs {
            command: SnuCommands::WaitToken { timeout_secs: 1 },
        }));
        assert_allowed(Commands::Snu(SnuArgs {
            command: SnuCommands::ListTables { timeout_secs: 1 },
        }));
        assert_allowed(Commands::Snu(SnuArgs {
            command: SnuCommands::GetRecord {
                table: "incident".to_string(),
                sys_id: "abc".to_string(),
                fields: None,
                timeout_secs: 1,
            },
        }));
        assert_allowed(Commands::Snu(SnuArgs {
            command: SnuCommands::Query {
                table: "incident".to_string(),
                query: None,
                fields: "sys_id".to_string(),
                limit: 10,
                order_by: None,
                timeout_secs: 1,
            },
        }));
        assert_allowed(Commands::Snu(SnuArgs {
            command: SnuCommands::Schema {
                table: "incident".to_string(),
                timeout_secs: 1,
            },
        }));
        assert_allowed(Commands::Snu(SnuArgs {
            command: SnuCommands::Screenshot {
                url: None,
                tab_id: None,
                out_path: None,
                timeout_secs: 1,
            },
        }));
    }

    #[test]
    fn read_only_denies_snu_mutating_commands() {
        assert_denied(Commands::Snu(SnuArgs {
            command: SnuCommands::UpdateRecord {
                table: "incident".to_string(),
                sys_id: "abc".to_string(),
                data: None,
                field: Some("state".to_string()),
                content: Some("2".to_string()),
                await_confirmation: false,
                timeout_secs: 1,
            },
        }));
        assert_denied(Commands::Snu(SnuArgs {
            command: SnuCommands::DeleteRecord {
                table: "incident".to_string(),
                sys_id: Some("abc".to_string()),
                query: None,
                confirm: false,
                limit: None,
                dry_run: false,
                timeout_secs: 1,
            },
        }));
        assert_denied(Commands::Snu(SnuArgs {
            command: SnuCommands::ExecuteBgScript {
                file: None,
                code: Some("gs.info('x')".to_string()),
                timeout_secs: 1,
            },
        }));
    }

    #[test]
    fn read_only_allows_local_management_but_denies_credential_export() {
        // Profile writes are local-only and now permitted in read-only mode.
        assert_allowed(Commands::Profile(ConfigArgs {
            command: ConfigCommands::Add {
                name: "dev".to_string(),
                instance: Some("https://dev.service-now.com".to_string()),
                auth_method: Some(CliAuthMethod::Basic),
                username: Some("admin".to_string()),
                client_id: None,
                oauth_grant_type: None,
                oauth_scope: None,
                oauth_redirect_host: None,
                oauth_redirect_port: None,
                oauth_redirect_path: None,
                cert_path: None,
                key_path: None,
                sso_login_url: None,
            },
        }));
        assert_allowed(Commands::Profile(ConfigArgs {
            command: ConfigCommands::UseProfile {
                name: "dev".to_string(),
            },
        }));
        assert_allowed(Commands::Profile(ConfigArgs {
            command: ConfigCommands::Sdk(ProfileSdkArgs {
                command: ProfileSdkCommands::Import {
                    alias: None,
                    all: true,
                    set_default: false,
                },
            }),
        }));
        // ...but exporting a stored credential into the on-disk now-sdk store is
        // a reusable-credential export and stays denied (same risk as `auth token`).
        assert_denied(Commands::Profile(ConfigArgs {
            command: ConfigCommands::Sdk(ProfileSdkArgs {
                command: ProfileSdkCommands::Export {
                    profile: "dev".to_string(),
                    alias: None,
                    set_default: false,
                },
            }),
        }));
        assert_denied(Commands::Profile(ConfigArgs {
            command: ConfigCommands::ExportNowSdk {
                profile: "dev".to_string(),
                alias: None,
                set_default: false,
            },
        }));
        // auth login/logout are local credential writes and now permitted.
        assert_allowed(Commands::Auth(AuthArgs {
            command: AuthCommands::Login {
                password: Some("secret".to_string()),
                password_stdin: false,
                token: None,
                token_stdin: false,
                client_secret: None,
                client_secret_stdin: false,
                session_cookie: None,
                session_cookie_stdin: false,
                no_browser: false,
                also_now_sdk: false,
                now_sdk_alias: None,
                set_now_sdk_default: false,
            },
        }));
        assert_allowed(Commands::Auth(AuthArgs {
            command: AuthCommands::Logout,
        }));
    }

    #[test]
    fn read_only_denies_mutating_and_sensitive_commands() {
        assert_denied(Commands::Auth(AuthArgs {
            command: AuthCommands::Token,
        }));
        assert_denied(Commands::Table(TableArgs {
            command: TableCommands::Update {
                table: "incident".parse().unwrap(),
                sys_id: "6816f79cc0a8016401c5a33be04be441".parse().unwrap(),
                data: Some("{}".to_string()),
            },
        }));
        assert_denied(Commands::Api(ApiArgs {
            command: ApiCommands::Post {
                path: "/api/x/do".to_string(),
                data: Some("{}".to_string()),
                header: vec![],
            },
        }));
        assert_denied(Commands::Script(ScriptArgs {
            command: ScriptCommands::Run {
                file: None,
                code: Some("gs.info('x')".to_string()),
                scope: "global".to_string(),
                endpoint: "/sys.scripts.do".to_string(),
                rollback: false,
                sandbox: false,
                scriptlet: false,
                quota_managed_transaction: false,
            },
        }));
    }

    #[test]
    fn read_only_request_policy_allows_get_only() {
        assert!(
            policy()
                .ensure_request_allowed(&Method::GET, "/api/now/table/incident")
                .is_ok()
        );
        assert!(
            policy()
                .ensure_request_allowed(&Method::POST, "/api/now/table/incident")
                .is_err()
        );
    }
}
