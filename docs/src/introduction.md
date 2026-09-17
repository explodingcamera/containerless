# Containerless

Containerless packages your application into a container image, ready to push to a registry.

> [!WARNING]
> Containerless is experimental and hasn't had a release yet. Commands and configuration may change.

## How it works

Build your application first, then describe the image in `containerless.toml`: its base, the files
to include, and runtime settings such as the entrypoint. Containerless bundles those files into an
image directly, without starting containers. You can run it as a regular command on your machine
or in CI, with no container infrastructure to set up for the build.

Run `containerless build` to save the image or `containerless publish` to push it to a registry.

See [Getting Started](./getting-started.md) for installation and a first build.
