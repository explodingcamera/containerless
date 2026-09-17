# completions

Generate a shell completion script for Containerless commands and options.

```sh
containerless completions SHELL
```

Supported shells are `bash`, `elvish`, `fish`, `powershell`, and `zsh`. The script is written to stdout.

## Fish

Install the script in Fish's user completion directory:

```sh
mkdir -p ~/.config/fish/completions
containerless completions fish > ~/.config/fish/completions/containerless.fish
```

## Bash

Load completions in the current Bash session:

```sh
source <(containerless completions bash)
```

To load them in future sessions, save the script and source it from your Bash configuration.

## Zsh

Save the script as `_containerless` in a directory on your Zsh `fpath` and load completions with
`compinit`:

```sh
containerless completions zsh > /path/on/fpath/_containerless
```

Regenerate saved scripts after upgrading Containerless to include new commands and options.
