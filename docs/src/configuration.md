# Configuration

The configuration describes your images: their base, files, runtime settings, and tags. A single
file can define several images and any layers they share.

## Configuration files

Containerless searches the current directory for `containerless.toml`, `containerless.json`,
`containerless.yaml`, or `containerless.yml`, in that order. Use `--file PATH` to read a different
file. The examples in these docs use TOML.

## Define an image

Each entry under `images` defines an image you can select by name on the command line:

```toml
[images.web]
base = "gcr.io/distroless/static-debian12:nonroot"
entrypoint = ["/usr/local/bin/web"]
user = "65532:65532"
workdir = "/srv"
tags = ["ghcr.io/example/web:latest"]

[[images.web.files]]
from = "dist/web"
to = "/usr/local/bin/web"
mode = "0755"

[images.web.env]
LOG_LEVEL = "info"

[images.web.labels]
"org.opencontainers.image.title" = "web"
```

Build this image with `containerless build web --output web.tar`.

## Runtime settings

These fields control how a container starts:

| Field | Example | Purpose |
| --- | --- | --- |
| `entrypoint` | `["/app"]` | Executable and fixed arguments |
| `command` | `["--port", "8080"]` | Default arguments passed to the entrypoint |
| `user` | `"65532:65532"` | User and optional group |
| `workdir` | `"/srv"` | Working directory |
| `stop_signal` | `"SIGTERM"` | Signal used to stop the container |
| `expose` | `["8080/tcp"]` | Ports described by the image |
| `volumes` | `["/data"]` | Paths intended for mounted volumes |
| `env` | `{ LOG_LEVEL = "info" }` | Environment variables |

The entrypoint and command are argument lists, not shell commands. With the examples above, the
container runs `/app --port 8080`. Exposing a port adds image metadata, it does not publish that
port on the host.

An image inherits its base's runtime settings unless you override them. Use an empty list to clear
an inherited `entrypoint`, `command`, `expose`, or `volumes` value. An empty string clears `user`,
`workdir`, or `stop_signal`. Environment variables are merged, with your values taking precedence.

## Labels, annotations, and tags

Use `labels` for metadata stored in the image configuration and `annotations` for metadata on the
OCI manifest. Both are string maps and merge with the base's values.

The `tags` list contains the full references used when publishing, such as
`ghcr.io/example/web:latest`. Tags belong to each image and are not inherited from a local base.
See [publish](./cli/publish.md) for authentication and command-line tags.

## Further configuration

- [Copying Files](./configuration/files.md) covers paths, filters, permissions, and symlinks.
- [Platforms](./configuration/platforms.md) explains how to package separate builds for each CPU
  architecture.
- [Base Images](./configuration/base-images.md) covers registry images and local bases.
- [Layers](./configuration/layers.md) covers shared files and combining layers.
