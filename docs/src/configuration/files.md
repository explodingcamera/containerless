# Copying Files

Use the `files` list to include your application, configuration, and assets in the image. Each entry
specifies a local source and its destination inside the image:

```toml
[[images.web.files]]
from = "dist/web"
to = "/usr/local/bin/web"
mode = "0755"
owner = "65532:65532"

[[images.web.files]]
from = "assets"
to = "/srv/assets"
```

## Source and destination paths

The `from` path is relative to the configuration file and cannot be absolute or contain `..`.
For example, if the configuration is in `deploy/containerless.toml`, `from = "dist/web"` refers to
`deploy/dist/web`.

The `to` path is an absolute path inside the image:

- For a file, `to` is its full destination path. `dist/web` copied to `/app` becomes `/app`.
- For a directory, its contents are copied into `to`. `assets/style.css` copied from `assets` to
  `/srv` becomes `/srv/style.css`.

Trailing slashes don't change this behavior. Set `preserve_paths = true` to keep the source's final
path component below the destination. In the directory example, the file becomes
`/srv/assets/style.css`.

Files are copied in the order listed. If two entries write to the same path, the later one wins.

## Filter directory contents

Use `include` and `exclude` to choose files within a source directory:

```toml
[[images.web.files]]
from = "assets"
to = "/srv/assets"
include = ["**/*.html", "**/*.css"]
exclude = ["drafts/**"]
```

Patterns match paths relative to `from`. `*` matches within a path component and `**` can match
across directories. Dotfiles are included unless you exclude them.

With no `include` patterns, all entries are eligible. Otherwise, an entry must match at least one
include pattern. Exclusions apply afterward. A filtered directory mapping that matches no entries
fails the build.

## Permissions and ownership

Set `mode` as a quoted octal string, such as `"0755"` for an executable or `"0644"` for a data file.
It applies to every entry copied by that mapping, including directories.

Without an explicit mode, regular files keep their permissions on Unix hosts. Directories default
to `0755` and symlinks to `0777`.

Ownership defaults to `0:0`. Use numeric IDs in `owner`, such as `"65532:65532"`. A single ID, such as
`"65532"`, sets both the user and group ID. This controls file ownership, while the image's `user`
setting controls which user runs the application.

## Symlinks

Containerless copies symlinks as links by default. Set `follow_symlinks = true` to copy their targets
instead. When following links within a source directory, a target that escapes that directory is
an error.

## Add files from the command line

Both `build` and `publish` accept repeated `--copy` options:

```sh
containerless build web --output web.tar --copy assets:/srv/assets
```

These sources are relative to your current working directory, rather than the configuration file.
They are added after the configured files and apply to every selected image and platform. Use the
configuration file for filters, permissions, or [platform-specific sources](./platforms.md).
