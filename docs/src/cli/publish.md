# publish

Build images and push them to the registries named in their tags.

```sh
containerless publish [IMAGE...] [OPTIONS]
```

## Choose tags

Set full image references in the configuration's `tags` list:

```toml
[images.web]
tags = ["ghcr.io/example/web:latest"]
```

Then publish the image:

```sh
containerless publish web
```

You can also add tags on the command line. These are added to any tags already in the configuration:

```sh
containerless publish web --tag ghcr.io/example/web:v1
```

When selecting several images, prefix each CLI tag with its image name:

```sh
containerless publish api worker \
  --tag api=ghcr.io/example/api:v1 \
  --tag worker=ghcr.io/example/worker:v1
```

Local base images' own tags are only published if you also select those images.

## Authentication

Containerless reads credentials from your Docker configuration, including configured credential
helpers. For example, you can authenticate to GitHub Container Registry with:

```sh
docker login ghcr.io
```

Those credentials are used for both pulling base images and pushing results.

For an HTTP registry, pass the host and port with `--plain-http`:

```sh
containerless publish web \
  --tag localhost:5000/web:dev \
  --plain-http localhost:5000
```

## Publish one executable

Use `--from` to package a single executable without a configuration file:

```sh
containerless publish \
  --from dist/app \
  --platform linux/amd64 \
  --tag ghcr.io/example/app:latest
```

This starts from `scratch`, copies the file to `/app` with mode `0755`, and sets `/app` as the
entrypoint. Use a statically linked Linux executable for this workflow. The platform defaults to
Linux on your host CPU architecture if you leave out `--platform`.

You must supply at least one tag. `--from` cannot be combined with `--file`, image names, or `--all`.
Add shared files with `--copy`, or use a [configuration file](../configuration.md) for a different
base or separate executables for each platform.

## Use metadata from CI

Read tags, labels, and annotations from Docker Metadata Action's JSON output with `--metadata-from`:

```sh
containerless publish web \
  --metadata-from metadata.json \
  --metadata-file build.json
```

Explicit CLI labels and annotations take precedence over imported values. Imported tags are added
alongside configured and CLI tags.

Like `build`, `publish` prints the top-level output index digest and supports `--digest-file` and
`--metadata-file`. See [Record build results](./build.md#record-build-results) for the difference
between output and published-image digests, and the
[shared build options](../cli.md#shared-build-options) for other flags.
