# Installation

## Quick install (macOS and Linux)

The quickest way to install `snow-cli` is with the install script:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.sh | bash
```

This detects your platform, downloads the latest release from GitHub, and places both `snow-cli` and `snow-cli-ro` in `~/.local/bin` (or `~/.snow-cli/bin` as a fallback).

If the install directory is not already on your `PATH`, the script will tell you how to add it.

### Custom install directory

```bash
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.sh | bash -s -- --install-dir /usr/local/bin
```

### Overwrite existing binaries

```bash
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.sh | bash -s -- --force
```

## Pre-built binaries

If you prefer to install manually, download the archive for your platform from the [GitHub releases page](https://github.com/ewatch/snow-cli/releases).

Supported platforms:

| Platform | Archive |
|----------|---------|
| macOS Intel | `snow-cli-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `snow-cli-aarch64-apple-darwin.tar.gz` |
| Windows x64 | `snow-cli-x86_64-pc-windows-msvc.zip` |

Extract the archive and place `snow-cli` (and optionally `snow-cli-ro`) in a directory on your `PATH`.

## Build from source

Clone the repository and build the release binary:

```bash
git clone https://github.com/ewatch/snow-cli.git
cd snow-cli
cargo build --release
```

The binaries are created at:

```text
target/release/snow-cli
target/release/snow-cli-ro
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
