# inspect

Show the images, platforms, and layer counts for a configuration before building an output.

```sh
containerless inspect [IMAGE...] [OPTIONS]
```

## Inspect an image

```sh
containerless inspect web
```

For a scratch image with one file layer targeting `linux/amd64`, the output looks like this:

```text
web
  linux/amd64 (1 layers)
```

The layer count covers layers added by Containerless, including those from local bases. External
base layers are separate.

Use `--all` to inspect every image, or pass several image names. To select platforms, use
`--platform`:

```sh
containerless inspect web --platform linux/amd64,linux/arm64
```

## JSON output

Add `--json` to see the resolved image definitions, including file mappings, runtime settings, and
references:

```sh
containerless inspect web --json
```

Inspection fetches external base images but does not read local source files or check whether they
match their filters. Those checks happen during the build.
