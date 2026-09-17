# Base Images

An image inherits its base's files and runtime settings. You can use an image from a registry,
reuse one defined in your configuration, or start with an empty image using `scratch`.

## Choose a base

The default base, `scratch`, starts with an empty filesystem. It works well for statically linked
executables and images containing only data. For a dynamically linked application, choose a base
with the libraries it needs, or include those libraries yourself.

To use an image from a registry, set `base` to its reference:

```toml
[images.app]
base = "gcr.io/distroless/cc-debian12:nonroot"
```

You can use a digest reference to keep the base fixed across builds. The base's runtime settings
are inherited as described in [Configuration](../configuration.md#runtime-settings).

## Use a local base

A local base lets images share both files and runtime settings:

```toml
[images.base]
user = "65532:65532"

[[images.base.files]]
from = "shared"
to = "/opt/shared"

[images.app]
base = { image = "base" }
entrypoint = ["/app"]

[[images.app.files]]
from = "dist/app"
to = "/app"
mode = "0755"
```

Building `app` includes `base`'s files and inherits its user setting, without publishing `base` first.

Publishing `app` only publishes its own tags. To publish both images, select both on the command
line or use `--all`, and give each image its own tags. Local bases cannot form a cycle.

For sharing files without inheriting runtime settings, use a [named layer](./layers.md).
