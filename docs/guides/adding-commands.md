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

### 5. Write tests

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
- [ ] Unit tests for handler logic
- [ ] Integration tests for CLI invocation
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes
