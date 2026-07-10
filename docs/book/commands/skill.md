# `skill`

Use `skill` to install agent skill bundles from local paths or URL-hosted manifests.

```bash
snow-cli skill <verb> [options]
```

All `skill` subcommands also accept the global flags from the [command overview](../commands.md).

## `skill install <source>`

Install a skill bundle from a local bundle directory, a local `skill.toml`, a `file://` URL, or an `http(s)` URL that points to a `skill.toml` manifest.

```bash
snow-cli skill install <source> [options]
```

Important options:

- `--target <codex|claude|agents>`: install into a known agent skill root
- `--target-dir <path>`: install into a custom root directory
- `--name <name>`: override the installed directory name
- `--pack <name>`: install a specific pack under `packs/<name>`; repeat to install several packs
- `--all-packs`: install every pack declared by the bundle

Examples:

```bash
snow-cli skill install ./skills/snow-cli --target agents
snow-cli skill install ./skills/snow-cli/skill.toml --target-dir ~/.agents/skills
snow-cli skill install https://example.com/skills/snow-cli/skill.toml --pack readonly
```
