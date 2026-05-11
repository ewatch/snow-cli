# `completions`

Use `completions` to generate shell completion scripts.

```bash
snow-cli completions <shell>
```

Supported shells:

- `bash`
- `zsh`
- `fish`
- `powershell`
- `elvish`

## Examples

Print a completion script to stdout:

```bash
snow-cli completions zsh
```

Save it to a file:

```bash
snow-cli completions bash > snow-cli.bash
snow-cli completions zsh > _snow-cli
snow-cli completions fish > snow-cli.fish
```

Typical installation patterns:

```bash
# bash
snow-cli completions bash > ~/.local/share/bash-completion/completions/snow-cli

# zsh
snow-cli completions zsh > ~/.zfunc/_snow-cli

# fish
snow-cli completions fish > ~/.config/fish/completions/snow-cli.fish
```

## Notes

- The command writes the generated completion script to stdout.
- Unlike most commands, the useful output here is the shell script itself, not structured JSON.
- Re-run the command after upgrading the CLI if the command surface changed.
