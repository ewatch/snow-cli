use std::sync::atomic::{AtomicU8, Ordering};

use reqwest::Method;
use thiserror::Error;

use crate::cli::args::{
    ApiCommands, AttachmentCommands, AuthCommands, CodesearchCommands, Commands, ConfigCommands,
    DataCommands, ImportSetCommands, ProfileSdkCommands, ScopeCommands, ScriptCommands,
    SeedCommands, SnuCommands, SnuContextCommands, TableCommands,
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
    LocalDaemon,
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
            Self::LocalDaemon => "local_daemon",
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

pub fn ensure_request_allowed(method: &Method, path: &str) -> Result<(), PolicyError> {
    active_policy().ensure_request_allowed(method, path)
}

pub fn ensure_raw_api_get_headers_allowed(
    extra_headers: &[(String, String)],
) -> Result<(), PolicyError> {
    if active_policy().mode() != PolicyMode::ReadOnly {
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

fn read_only_command_decision(command: &Commands) -> PolicyDecision {
    match command {
        Commands::Profile(args) => match &args.command {
            ConfigCommands::ListProfiles
            | ConfigCommands::FindProfile { .. }
            | ConfigCommands::Current
            | ConfigCommands::Show
            | ConfigCommands::ListNowSdkProfiles => PolicyDecision::Allow,
            ConfigCommands::Sdk(sdk_args) => match &sdk_args.command {
                ProfileSdkCommands::List => PolicyDecision::Allow,
                ProfileSdkCommands::Import { .. } | ProfileSdkCommands::Export { .. } => deny(
                    "profile sdk import/export",
                    CommandCapability::LocalConfigWrite,
                    "read-only policy does not allow profile import or export operations",
                ),
            },
            ConfigCommands::Init { .. }
            | ConfigCommands::Add { .. }
            | ConfigCommands::Edit { .. }
            | ConfigCommands::SetProfile { .. }
            | ConfigCommands::ImportNowSdk { .. }
            | ConfigCommands::ExportNowSdk { .. }
            | ConfigCommands::UseProfile { .. }
            | ConfigCommands::DeleteProfile { .. } => deny(
                "profile write",
                CommandCapability::LocalConfigWrite,
                "read-only policy does not allow profile configuration changes",
            ),
        },
        Commands::Auth(args) => match &args.command {
            AuthCommands::Status => PolicyDecision::Allow,
            AuthCommands::Token => deny(
                "auth token",
                CommandCapability::CredentialExport,
                "read-only policy does not allow exporting reusable credentials",
            ),
            AuthCommands::Login { .. } | AuthCommands::Logout => deny(
                "auth login/logout",
                CommandCapability::LocalCredentialWrite,
                "read-only policy does not allow credential changes",
            ),
        },
        Commands::Table(args) => match &args.command {
            TableCommands::List { .. }
            | TableCommands::Get { .. }
            | TableCommands::Schema { .. } => PolicyDecision::Allow,
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
            | SnuCommands::Query { .. }
            | SnuCommands::Schema { .. }
            | SnuCommands::Slash { .. }
            | SnuCommands::Tab(_)
            | SnuCommands::Screenshot { .. } => PolicyDecision::Allow,
            SnuCommands::UpdateRecord { .. } => deny(
                "snu update-record",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow record updates through SN-Utils",
            ),
            SnuCommands::UpdateRecordBatch { .. } => deny(
                "snu update-record-batch",
                CommandCapability::RemoteWrite,
                "read-only policy does not allow record updates through SN-Utils",
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
            SnuCommands::Daemon(_) => deny(
                "snu daemon",
                CommandCapability::LocalDaemon,
                "read-only policy does not allow starting/stopping the bridge daemon",
            ),
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
                table: "incident".to_string(),
                query: None,
                fields: None,
                limit: Some(1),
                order_by: None,
            },
        }));
        assert_allowed(Commands::Table(TableArgs {
            command: TableCommands::Get {
                table: "incident".to_string(),
                sys_id: "abc".to_string(),
                fields: None,
            },
        }));
        assert_allowed(Commands::Table(TableArgs {
            command: TableCommands::Schema {
                table: "incident".to_string(),
                extended: false,
                include_inherited: false,
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
    fn read_only_denies_mutating_and_sensitive_commands() {
        assert_denied(Commands::Auth(AuthArgs {
            command: AuthCommands::Token,
        }));
        assert_denied(Commands::Table(TableArgs {
            command: TableCommands::Update {
                table: "incident".to_string(),
                sys_id: "abc".to_string(),
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
