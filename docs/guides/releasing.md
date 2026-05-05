# Releasing snow-cli

snow-cli uses [`cargo-dist`](https://github.com/axodotdev/cargo-dist) to build
release archives and installers for GitHub Releases.

## Release assets

The release workflow builds these binary archives:

| Platform | Target | Runner |
| --- | --- | --- |
| macOS Intel | `x86_64-apple-darwin` | `macos-latest` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `macos-latest` |
| Windows x64 | `x86_64-pc-windows-msvc` | `windows-latest` |

It also generates cargo-dist shell and PowerShell installers when supported by
the cargo-dist configuration.

## Creating a release

1. Update the package version in `Cargo.toml`.
2. Commit the version change.
3. Create and push a matching `v*` tag:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. The `Release` GitHub Actions workflow builds the archives and publishes them
   to a GitHub Release for that tag.

You can also create a release from the GitHub UI. The workflow listens for the
`release.published` event and uploads the same cargo-dist assets to the release.

## Manual rebuild

If a release asset build needs to be rerun, start the `Release` workflow manually
from GitHub Actions and provide the release tag, for example `v0.2.0`.

## Local validation

To validate the cargo-dist configuration locally:

```bash
cargo install cargo-dist --version 0.28.0 --locked
cargo dist plan
```

To build a specific archive locally:

```bash
cargo dist build --artifacts=local --target x86_64-apple-darwin --tag v0.2.0
```
