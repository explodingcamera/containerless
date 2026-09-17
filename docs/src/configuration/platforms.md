# Platforms

A multi-platform image contains a build for each supported platform under one tag. When a container
runtime pulls it, it selects the build that matches the machine's operating system and architecture.

Platform names use OCI notation, such as `linux/amd64`, `linux/arm64`, or `linux/arm/v7`.

## Package multiple builds

Build your application for each target, then use a map in `from` to choose the matching file:

```toml
[images.web]
entrypoint = ["/app"]

[[images.web.files]]
from = { "linux/amd64" = "dist/amd64/web", "linux/arm64" = "dist/arm64/web" }
to = "/app"
mode = "0755"

[[images.web.files]]
from = "assets"
to = "/srv/assets"
```

This builds images for both platforms, with the same `assets` directory in each. Publishing creates
an OCI index that groups them under one tag:

```sh
containerless publish web --tag ghcr.io/example/web:latest
```

This example uses the default scratch base, so the executables need to be statically linked.

## Choose platforms explicitly

Set `platforms` on the image to specify which platforms to build:

```toml
[images.web]
platforms = ["linux/amd64", "linux/arm64"]
```

You can override that list for an invocation with `--platform`. Repeat the option or separate values
with commas:

```sh
containerless build web --platform linux/amd64 --output web-amd64.tar
containerless inspect web --platform linux/amd64,linux/arm64
```

Every file mapping must provide a source for each selected platform. A plain string source applies
to all platforms. A source map can include a `default` fallback:

```toml
[[images.web.files]]
from = { "linux/arm64" = "config/arm64", default = "config/common" }
to = "/etc/web"
```

The `default` key supplies a fallback source without adding a platform to the build.

## How defaults are chosen

Without a CLI override, an image's explicit `platforms` list takes precedence. Otherwise,
Containerless collects platform keys from its file sources, named layers, and local base chain.

If there are no platform-specific sources, it uses an explicit platform list from a local base when
available. Otherwise, it defaults to Linux on the host CPU architecture.

An external base is pulled separately for each output platform. A local base must provide the
platforms needed by its child image.
