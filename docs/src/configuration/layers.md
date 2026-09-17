# Layers

Layers group the files that make up an image. You can define named layers to share files between
images, or combine layers when you want a single filesystem layer.

An image starts with its [base image's](./base-images.md) layers, followed by named layers in the
order you list them, then the image's own `files` as a final layer.

## Share a named layer

Group files under `layers` when several images need the same content:

```toml
[[layers.assets.files]]
from = "assets"
to = "/srv/assets"

[images.web]
layers = ["assets"]

[images.preview]
layers = ["assets"]
```

Named layers use the same [file options](./files.md) as an image's `files` list.

## Combine layers

To combine layers, set one of these options on the image:

- `squash = true` combines the layers added by that configured image, keeping its base layers intact.
- `flatten = true` applies all layers, including the base, and writes the resulting filesystem as
  a single layer. This also applies any file deletions recorded in the base layers.

The command-line `--squash` option combines all layers added by Containerless, including local base
layers and files from `--copy`. External base layers remain separate. `--flatten` combines the
entire filesystem.

`--squash` and `--flatten` cannot be passed together, and a configured image cannot enable both
`squash` and `flatten`.
