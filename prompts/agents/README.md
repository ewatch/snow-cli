# Harness-Independent Agents

The role prompts in this directory are the single source of truth. They contain no model, tool, or harness-specific instructions.

## Available harnesses

| Model family | Supported harnesses |
| --- | --- |
| DeepSeek V4 Flash | OpenCode only |
| Claude | Claude Code only |
| OpenAI | Pi, OpenCode, or Codex |

## Suggested release pipeline

Run each role with a supported harness. Omit `--model` to use that harness's
configured default model.

```sh
scripts/run-agent claude reviewer --model opus -- "Review the current branch against main."
scripts/run-agent opencode e2e-tester --model <configured-deepseek-v4-flash> -- "Run the release command matrix for version 0.4.0."
scripts/run-agent pi documentation-maintainer -- "Update the installation guide using artifacts/e2e/0.4.0/."
scripts/run-agent claude release-manager -- "Prepare a release-readiness report for version 0.4.0."
```

The launcher uses the subscriptions and credentials already configured for the
selected harness. Do not use Claude through Pi or OpenCode, and do not use
DeepSeek V4 Flash outside OpenCode.

The reviewer is launched with each harness's read-only policy where supported. Other roles retain the harness's normal confirmation behavior; their prompts prohibit publishing or destructive release actions without an explicit request.

For releases, run `reviewer`, `e2e-tester`, `documentation-maintainer`, then
`release-manager`. See `docs/guides/releasing.md` for the evidence and re-run
requirements.
