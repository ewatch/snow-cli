# Phase 5 — Polish and Distribution

Final polish, advanced auth methods, CI/CD, and distribution.

## Work Items

- [ ] Shell completions
  - [ ] `completions` command using `clap_complete`
  - [ ] Support bash, zsh, fish, PowerShell
  - [ ] Installation instructions in README
- [ ] Interactive setup wizard
  - [ ] `config init` guides user through first-time setup
  - [ ] Instance URL, auth method, credentials
  - [ ] Stores credentials in keychain
- [ ] Advanced authentication
  - [ ] mTLS — client certificate configuration in reqwest
  - [ ] SSO/SAML — browser flow with local callback server
  - [ ] Tests for each
- [ ] CI/CD (GitHub Actions)
  - [ ] Build matrix: Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows
  - [ ] Run tests on all platforms
  - [ ] Release workflow: build binaries, create GitHub release
  - [ ] Cargo clippy and fmt checks
- [ ] Homebrew distribution
  - [ ] Create Homebrew formula
  - [ ] Tap repository setup
  - [ ] Automated formula update on release
- [ ] Documentation
  - [ ] README with installation, quick start, examples
  - [ ] Usage guide for agents (machine-readable command reference)
  - [ ] Contributing guide
