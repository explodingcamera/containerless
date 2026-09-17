# Getting Started

## Install from source

With Rust installed, install the CLI from GitHub:

```sh
cargo install --git https://github.com/explodingcamera/containerless --locked containerless
```

Run `containerless --help` to see the available commands.

## Build your application

Build your application for Linux using your usual build tools. This example uses a statically
linked executable at `dist/app`.

## Configure the image

Create `containerless.toml` in your project directory. Replace `ghcr.io/example/app:latest` with the
registry and repository you want to publish to:

```toml
[images.app]
base = "scratch"
entrypoint = ["/app"]
tags = ["ghcr.io/example/app:latest"]

[[images.app.files]]
from = "dist/app"
to = "/app"
mode = "0755"
```

This puts the executable at `/app` and runs it when the container starts. If your application needs
shared libraries, choose a [base image](./configuration/base-images.md) that provides them instead
of `scratch`.

## Publish an image

Containerless uses credentials from your Docker configuration. If you haven't logged in to your
registry, do that first. For GitHub Container Registry:

```sh
docker login ghcr.io
```

Build and publish the configured image:

```sh
containerless publish
```

Containerless selects `app`, builds the image, and pushes it to `ghcr.io/example/app:latest`.
By default, it targets Linux on your host's CPU architecture. If you built for a different
architecture, set the image's `platforms` list to match.

## Next steps

- [Copying Files](./configuration/files.md) covers assets, permissions, and file filters.
- [Platforms](./configuration/platforms.md) shows how to publish builds for multiple architectures.
- [publish](./cli/publish.md) covers additional tags and CI metadata.
- [build](./cli/build.md) explains how to save an image locally.
