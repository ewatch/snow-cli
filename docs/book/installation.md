# Installation

## Build from source

Clone the repository and build the release binary:

```bash
git clone https://github.com/ewatch/snow-cli.git
cd snow-cli
cargo build --release
```

The binary is created at:

```text
target/release/snow-cli
```

Run:

```bash
./target/release/snow-cli --help
```

## Development build

For local development, run through Cargo:

```bash
cargo run -- --help
cargo run -- table list --help
```

## Verify the project

Before contributing changes, run:

```bash
cargo fmt -- --check
cargo test
cargo clippy -- -D warnings
```

## Configuration file

`snow-cli` stores profile configuration in:

```text
~/.servicenow/config.toml
```

Secrets are stored in the operating system keychain where possible, not in plaintext config files.
