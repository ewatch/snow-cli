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
- [x] Homebrew distribution
  - [x] Create Homebrew formula
  - [x] Tap repository setup
  - [x] Automated formula update on release
- [ ] CLI UX improvements (agent-browser-inspired usability, adapted for snow-cli)
  - [x] Help that teaches
    - [x] Add practical examples (`after_help`) for top-level and high-traffic commands
    - [x] Add "Common workflows" section to `snow-cli --help` (init, login, list/create/update)
    - [x] Align command descriptions with real behavior (e.g., `config init` wording)
  - [x] Actionable and consistent errors
    - [x] Standardize errors to include concrete next-step command suggestions
    - [x] Add common typo/missing-input guidance (profile not found, missing `--data`, etc.)
    - [x] Replace `todo!()` placeholders in incomplete commands with graceful errors
  - [ ] Ergonomics and aliases
    - [ ] Add safe aliases for frequently used commands (e.g., `cfg`, `cs`, `ls`, `rm`)
    - [ ] Add a global non-interactive confirmation flag pattern (`--yes`) where applicable
    - [ ] Review short flags for high-frequency options without introducing ambiguity
  - [ ] Onboarding and diagnostics
    - [ ] Add `snow-cli doctor` command for config/auth/connectivity checks with fix hints
    - [ ] Improve first-run guidance when config/profile is missing
    - [ ] Evaluate optional `config init --interactive` mode while keeping non-interactive support
  - [ ] Output UX
    - [ ] Keep default output as JSON (agent-first, script-safe)
    - [ ] Add optional human-friendly table output mode (`--output table`)
    - [ ] Keep JSON deterministic; add optional pretty-print mode for humans
    - [ ] Standardize success/status output envelope across commands
  - [ ] Test and docs coverage for UX
    - [ ] Add integration tests for help text and example discoverability
    - [ ] Add integration tests for improved error messages and alias compatibility
    - [ ] Update README and guides with UX-focused usage examples
- [ ] Documentation
  - [ ] README with installation, quick start, examples
  - [ ] Usage guide for agents (machine-readable command reference)
  - [ ] Contributing guide
