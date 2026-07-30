# Containerless

> Work in progress

Containerless makes building minimal, multi-platform container images simple.

It is a lightweight OCI image builder for local build outputs. Point it at your files, choose a base
image and runtime metadata, then publish to a registry or export an archive.

Containerless is a good fit for small images, multi-platform releases, and lightweight CI. It can
assemble each platform from separately built artifacts without a daemon or privileged setup.

## See Also

- [Buildah](https://buildah.io/) - daemonless OCI image builder.
- [ko](https://ko.build/) - container image builder for Go applications.
- [Kaniko](https://github.com/GoogleContainerTools/kaniko) - containerized Dockerfile image builder.
