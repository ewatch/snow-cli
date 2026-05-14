# Adding a New Command

This guide walks through adding a new noun-verb command to snow-cli.

## Steps

### 1. Define the clap subcommand

Edit `src/cli/args.rs` and add your noun as a variant in the `Commands` enum,
then define the verb subcommands:

```rust
#[derive(Subcommand)]
pub enum Commands {
    // ... existing commands ...

    /// Manage change requests
    Change(ChangeArgs),
}

#[derive(Args)]
pub struct ChangeArgs {
    #[command(subcommand)]
    pub command: ChangeCommands,
}

#[derive(Subcommand)]
pub enum ChangeCommands {
    /// List change requests
    List {
        #[arg(long)]
        limit: Option<usize>,

        #[arg(long)]
        query: Option<String>,
    },

    /// Get a change request by number
    Get {
        /// Change request number (e.g., CHG0010001)
        number: String,
    },
}
```

### 2. Create the command handler

Create `src/cli/commands/change.rs`:

```rust
use crate::client::SnowClient;
use crate::error::CliError;

pub async fn handle_list(
    client: &SnowClient,
    limit: Option<usize>,
    query: Option<String>,
) -> Result<(), CliError> {
    // Implementation here
    Ok(())
}

pub async fn handle_get(
    client: &SnowClient,
    number: &str,
) -> Result<(), CliError> {
    // Implementation here
    Ok(())
}
```

### 3. Register in the command module

Add `pub mod change;` to `src/cli/commands/mod.rs`.

### 4. Wire up in main dispatch

In `src/main.rs` (or wherever command dispatch happens), add the match arm:

```rust
Commands::Change(args) => match args.command {
    ChangeCommands::List { limit, query } => {
        commands::change::handle_list(&client, limit, query).await
    }
    ChangeCommands::Get { number } => {
        commands::change::handle_get(&client, &number).await
    }
},
```

### 5. Classify read-only policy behavior

Every command must be explicitly classified in `src/policy.rs`. If the command
can mutate ServiceNow, export reusable credentials, or change local config or
credentials, deny it under `PolicyMode::ReadOnly`. If the command is audited as
read-only and should be available to agent harnesses, also add it to
`src/cli/readonly_args.rs` so it appears in `snow-cli-ro` help and completions.

`api get` is intentionally allowed by HTTP convention, but method override
headers remain blocked in read-only mode.

### 6. Write tests

Add integration tests in `tests/test_change.rs`:

```rust
use assert_cmd::Command;

#[test]
fn test_change_list_help() {
    Command::cargo_bin("snow-cli")
        .unwrap()
        .args(["change", "list", "--help"])
        .assert()
        .success();
}
```

Add unit tests in `src/cli/commands/change.rs` for the handler logic.

## Checklist

- [ ] Subcommand defined in `src/cli/args.rs`
- [ ] Handler module created in `src/cli/commands/`
- [ ] Module registered in `src/cli/commands/mod.rs`
- [ ] Dispatch wired up in main
- [ ] Read-only policy decision added in `src/policy.rs`
- [ ] Added to `src/cli/readonly_args.rs` if it should be exposed by `snow-cli-ro`
- [ ] Unit tests for handler logic
- [ ] Integration tests for CLI invocation
- [ ] Read-only allow/deny tests updated
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes
