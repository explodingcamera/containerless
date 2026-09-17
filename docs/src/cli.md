# CLI Reference

Use `build` to export images, `publish` to push them to a registry, and `inspect` to check the
configuration before building.

| Command | Purpose |
| --- | --- |
| [build](./cli/build.md) | Build images and save or publish them |
| [publish](./cli/publish.md) | Build images and push them to a registry |
| [inspect](./cli/inspect.md) | Show the resolved image configuration |
| [completions](./cli/completions.md) | Generate a shell completion script |

Run `containerless --help` or `containerless COMMAND --help` for the full option list in your
installed version.

## Select a configuration

By default, Containerless looks for a [configuration file](./configuration.md) in the current
directory. Use `-f` or `--file` to select another file:

```sh
containerless --file deploy/containerless.toml build web --output web.tar
```

## Select images

The `build`, `publish`, and `inspect` commands select the image automatically when your
configuration has only one. If you have several, pass their names or use `--all`:

```sh
containerless build api worker --output services.tar
containerless inspect --all
```

Use either names or `--all`, not both. Any local base images are included as dependencies, but only
selected images receive their own output entries and publication tags.

## Shared build options

These options are available on both `build` and `publish`:

| Option | Purpose |
| --- | --- |
| `--platform PLATFORM` | Choose platforms, repeat the option or separate values with commas |
| `--copy SOURCE:DESTINATION` | Add a file or directory after the configured files |
| `-t`, `--tag [IMAGE=]REFERENCE` | Add a tag to publish |
| `--label KEY=VALUE` | Set an image label |
| `--annotation KEY=VALUE` | Set an OCI annotation |
| `--squash` | Combine layers added by Containerless |
| `--flatten` | Combine the entire filesystem into one layer |
| `--plain-http REGISTRY` | Use HTTP for a registry host |
| `--digest-file PATH` | Write the top-level output index digest |
| `--metadata-file PATH` | Write build results as JSON |
| `--metadata-from PATH` | Read tags, labels, and annotations from Docker Metadata Action JSON |

You can repeat `--copy`, `--tag`, `--label`, `--annotation`, and `--plain-http`. Command-line labels
and annotations override matching configured values, and command-line tags are added to configured
tags.

See [Copying Files](./configuration/files.md), [Platforms](./configuration/platforms.md), and
[Layers](./configuration/layers.md) for how these options affect a build.
