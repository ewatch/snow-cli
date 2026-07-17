# Installation

Install `snow-cli` and its read-only companion, `snow-cli-ro`, with Homebrew,
the platform installer, or a source build. The platform installer selects the
latest GitHub release for your platform and asks before it downloads anything.

## Homebrew (macOS and Linux)

The simplest way to install on macOS or Linux is via the Homebrew tap:

```sh
brew tap ewatch/tap
brew install snow-cli
```

Upgrade later with:

```sh
brew upgrade snow-cli
```

This installs both `snow-cli` and `snow-cli-ro`. Homebrew keeps the formula up to
date automatically, so `brew upgrade` always fetches the latest release.

## Quick install

### macOS and Linux

Copy the command below into your terminal. The script shows exactly what it will do before downloading anything:

```bash
curl -fsSL https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.sh | bash
```

You will see a plan like this and be asked to confirm:

```text
Plan:
  Download: https://github.com/ewatch/snow-cli/releases/download/v0.6.0/snow-cli-0.6.0-aarch64-apple-darwin.tar.xz
  Release:  v0.6.0
  Install to: /Users/you/.local/bin
  Binaries: snow-cli, snow-cli-ro

Proceed? [Y/n]
```

#### Skip the confirmation

If you are running this in CI or prefer no prompts:

```bash
FORCE=1 curl -fsSL https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.sh | bash
```

#### Use a different directory

```bash
INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.sh | bash
```

### Windows

Open PowerShell and run:

```powershell
irm https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.ps1 | iex
```

The script shows the same plan-and-confirm flow and installs the Windows
executables in `$env:LOCALAPPDATA\snow-cli\bin` by default. To skip the prompt
in automation:

```powershell
$env:FORCE = "1"; irm https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.ps1 | iex
```

### What the script does (in plain English)

1. Finds the latest release on GitHub.
2. On macOS and Linux, detects the operating system and CPU architecture; on
   Windows, selects the x86_64 Windows archive for a 64-bit operating system.
3. Downloads the matching archive (`tar.xz` for macOS/Linux, `zip` for Windows).
4. Extracts it to a temporary folder.
5. Copies `snow-cli` and `snow-cli-ro` (the `.exe` files on Windows) into the
   install directory.
6. Tells you if the directory is missing from your `PATH` and how to add it.

No registry changes, no admin rights required by default, and the archive is deleted automatically.

## Manual install (pre-built binaries)

If you prefer to install by hand, download the archive for your platform from the [GitHub releases page](https://github.com/ewatch/snow-cli/releases).

| Platform | Archive |
|----------|---------|
| macOS Intel | `snow-cli-<version>-x86_64-apple-darwin.tar.xz` |
| macOS Apple Silicon | `snow-cli-<version>-aarch64-apple-darwin.tar.xz` |
| Linux x64 | `snow-cli-<version>-x86_64-unknown-linux-gnu.tar.xz` |
| Linux ARM64 | `snow-cli-<version>-aarch64-unknown-linux-gnu.tar.xz` |
| Windows x64 | `snow-cli-<version>-x86_64-pc-windows-msvc.zip` |

> The "unknown" in the Linux filename is not a placeholder — it is the standard Rust target triple vendor field (x86_64-**unknown**-linux-gnu), which is how the release archives are named.

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
cargo run --bin snow-cli -- --help
cargo run --bin snow-cli -- table list --help
cargo run --bin snow-cli-ro -- --help
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
