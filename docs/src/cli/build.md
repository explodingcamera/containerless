# build

Build images from your configuration and save them locally, push them to a registry, or both.

```sh
containerless build [IMAGE...] [OPTIONS]
```

Every build needs `--output`, `--push`, or both. Containerless has no local image store.

## Save an image

Use `-o` or `--output` to write an OCI archive:

```sh
containerless build web --output web.tar
```

To write an OCI image layout directory instead, add `--format oci-dir`:

```sh
containerless build web --output web-image --format oci-dir
```

The directory contains `oci-layout`, `index.json`, and the image blobs under `blobs/sha256/`.
The default `--format oci` packages that layout as a tar archive.

A single output can contain several selected images:

```sh
containerless build api worker --output services.tar
```

## Save and publish together

Add `--push` to publish to the configured or command-line tags:

```sh
containerless build web \
  --output web.tar \
  --push \
  --tag ghcr.io/example/web:v1
```

For a registry-only build, use `--push` without `--output`, or use
[`containerless publish`](./publish.md).

## Record build results

A successful build prints the top-level output index digest to stdout. Use `--digest-file` to save
it, or `--metadata-file` for more detail:

```sh
containerless build web \
  --output web.tar \
  --digest-file digest.txt \
  --metadata-file build.json
```

The JSON contains `index_digest` for the output index and `image_digests`, a map from configured
image names to their manifest or multi-platform index digests. The top-level output index may
contain several images, so its digest can differ from the digest attached to an individual
published tag.

## Reproducible timestamps

Containerless uses a timestamp of `0` for generated archive entries by default. To choose another
timestamp, set `SOURCE_DATE_EPOCH` to a non-negative number of seconds since the Unix epoch:

```sh
SOURCE_DATE_EPOCH=1700000000 containerless build web --output web.tar
```

For reproducible builds, also pin the base image by digest and keep input files and permissions
consistent.

See the [shared build options](../cli.md#shared-build-options) for file copies, platforms, labels,
and layer settings.
