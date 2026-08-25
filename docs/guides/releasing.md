# Releasing snow-cli

snow-cli uses the repository's `Release` GitHub Actions workflow to build and
package release archives. macOS and Windows build with Cargo on native runners;
Linux targets build through `cross`.

## Release assets

The release workflow publishes these binary archives:

| Platform | Target | Runner |
| --- | --- | --- |
| macOS Intel | `x86_64-apple-darwin` | `macos-latest` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `macos-latest` |
| Linux x64 | `x86_64-unknown-linux-gnu` | `ubuntu-latest` via `cross` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | `ubuntu-latest` via `cross` |
| Windows x64 | `x86_64-pc-windows-msvc` | `windows-latest` |

It also publishes a consolidated `SHA256SUMS` file covering the archive assets above.

Shell and PowerShell installer scripts are intentionally not attached to GitHub
releases at this time because many environments treat downloaded scripts as
untrusted. GitHub still adds its standard `Source code (zip)` and
`Source code (tar.gz)` archives automatically.

## Creating a release

### Validation gates

Complete these gates for the release candidate before creating a tag or GitHub
release:

1. Run the reviewer against the release fixed point, specification, and
   repository standards.
2. Run the E2E command matrix and save sanitized evidence under
   `artifacts/e2e/<version>/`. Each scenario records the exact command,
   arguments, exit code, sanitized stdout and stderr, assertion result, and
   harness/model metadata.
3. Update user documentation from successful E2E artifacts only. Examples must
   not contain credentials, instance URLs, sys_ids, or unstable generated
   values.
4. Verify the final candidate's version metadata, release notes, release
   workflow configuration, host release build, tests, formatting, and lint checks.

The local SN-Utils bridge protocol tests are required. Live ServiceNow or
browser-helper smoke tests are reported separately as passed, failed, or
unavailable; an unavailable test does not count as a pass.

Any code or behavior change after review restarts review, E2E testing, and
documentation for the new candidate.

### Publish after approval

After the release manager declares the candidate ready and a human explicitly
approves publication:

1. Update the package version in `Cargo.toml`.
2. Commit the version change.
3. Create and push a matching `v*` tag:

   ```bash
   git tag v0.3.1
   git push origin v0.3.1
   ```

4. The `Release` GitHub Actions workflow builds the archives and publishes them
   to a GitHub Release for that tag.

## Manual rebuild

If a release asset build needs to be rerun, start the `Release` workflow manually
from GitHub Actions and provide the release tag, for example `v0.3.1`.

## Local validation

Build both binaries for the host platform with the locked dependency graph:

```bash
cargo build --release --locked --bins
target/release/snow-cli --version
target/release/snow-cli-ro --version
```

The `Release` workflow is the authoritative cross-platform packaging check. It
builds the configured target matrix, verifies both binaries exist in every
archive, checks versioned asset names, and produces `SHA256SUMS` before
publishing the draft release.
