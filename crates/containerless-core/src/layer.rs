use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use flate2::{Compression, GzBuilder};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use sha2::{Digest, Sha256};
use tar::{Builder, EntryType, Header};
use tempfile::NamedTempFile;
use tracing::{debug, info};
use walkdir::WalkDir;

use crate::{BuildError, FileMapping, Layer};

pub(super) struct LayerBlob {
    pub digest: String,
    pub size: u64,
    pub diff_id: String,
}

#[derive(Clone)]
pub(super) struct Entry {
    pub source: Option<PathBuf>,
    pub data: Option<Vec<u8>>,
    pub link: Option<PathBuf>,
    pub kind: EntryType,
    pub mode: u32,
    pub uid: u64,
    pub gid: u64,
    pub device_major: Option<u32>,
    pub device_minor: Option<u32>,
}

impl Layer {
    pub(super) fn build(&self, blobs: &Path, timestamp: u64) -> Result<LayerBlob, BuildError> {
        let mut entries = BTreeMap::new();
        for mapping in &self.files {
            mapping.collect_into(&mut entries)?;
        }
        write_layer(entries, blobs, timestamp)
    }
}

pub(super) fn write_layer(
    entries: BTreeMap<String, Entry>,
    blobs: &Path,
    timestamp: u64,
) -> Result<LayerBlob, BuildError> {
    info!(entries = entries.len(), "building layer");
    let temporary = NamedTempFile::new_in(blobs)?;
    let compressed = DigestWriter::new(temporary);
    let gzip = GzBuilder::new()
        .mtime(u32::try_from(timestamp).unwrap_or(0))
        .operating_system(255)
        .write(compressed, Compression::default());
    let uncompressed = DigestWriter::new(gzip);
    let mut archive = Builder::new(uncompressed);

    for (path, entry) in entries {
        let mut header = Header::new_gnu();
        header.set_entry_type(entry.kind);
        header.set_mode(entry.mode);
        header.set_uid(entry.uid);
        header.set_gid(entry.gid);
        header.set_mtime(timestamp);
        if let Some(major) = entry.device_major {
            header.set_device_major(major)?;
        }
        if let Some(minor) = entry.device_minor {
            header.set_device_minor(minor)?;
        }
        if entry.kind.is_file() {
            if let Some(source) = &entry.source {
                header.set_size(fs::metadata(source)?.len());
                header.set_cksum();
                archive.append_data(&mut header, path, File::open(source)?)?;
            } else {
                let data = entry.data.as_deref().unwrap_or_default();
                header.set_size(data.len() as u64);
                header.set_cksum();
                archive.append_data(&mut header, path, data)?;
            }
        } else if entry.kind.is_symlink() || entry.kind.is_hard_link() {
            header.set_size(0);
            header.set_cksum();
            archive.append_link(&mut header, path, entry.link.as_ref().unwrap())?;
        } else {
            header.set_size(0);
            header.set_cksum();
            archive.append_data(&mut header, path, io::empty())?;
        }
    }

    archive.finish()?;
    let uncompressed = archive.into_inner()?;
    let (gzip, diff_id, _) = uncompressed.finish();
    let compressed = gzip.finish()?;
    let (temporary, digest, size) = compressed.finish();
    let destination = blobs.join(&digest);
    if destination.exists() {
        temporary.close()?;
    } else {
        temporary
            .persist(&destination)
            .map_err(|error| error.error)?;
    }
    debug!(digest = %digest, size, diff_id = %diff_id, "wrote layer blob");

    Ok(LayerBlob {
        digest: format!("sha256:{digest}"),
        size,
        diff_id: format!("sha256:{diff_id}"),
    })
}

impl FileMapping {
    pub(super) fn collect_into(
        &self,
        entries: &mut BTreeMap<String, Entry>,
    ) -> Result<(), BuildError> {
        let metadata = if self.follow_symlinks {
            fs::metadata(&self.source)
        } else {
            fs::symlink_metadata(&self.source)
        }
        .map_err(|error| {
            BuildError::Invalid(format!("cannot read {}: {error}", self.source.display()))
        })?;
        let destination = normalize_destination(&self.destination)?;
        let prefix = if self.preserve_paths {
            self.source.file_name().map(PathBuf::from).ok_or_else(|| {
                BuildError::Invalid(format!(
                    "cannot preserve path for {}",
                    self.source.display()
                ))
            })?
        } else {
            PathBuf::new()
        };

        if !metadata.is_dir() {
            let destination = join_destination(&destination, &prefix)?;
            if destination.is_empty() {
                return Err(BuildError::Invalid(
                    "a file cannot be copied to the image root".to_owned(),
                ));
            }
            insert_entry(
                entries,
                destination,
                self.entry_for(&self.source, &metadata)?,
            );
            return Ok(());
        }

        let directory_destination = join_destination(&destination, &prefix)?;
        if !directory_destination.is_empty() {
            insert_entry(
                entries,
                directory_destination,
                self.entry_for(&self.source, &metadata)?,
            );
        }

        let include = build_globs(&self.include)?;
        let exclude = build_globs(&self.exclude)?;
        let source_root = self
            .follow_symlinks
            .then(|| self.source.canonicalize())
            .transpose()?;
        let mut matched = false;
        for result in WalkDir::new(&self.source)
            .min_depth(1)
            .follow_links(self.follow_symlinks)
            .sort_by_file_name()
        {
            let walked = result.map_err(|error| BuildError::Invalid(error.to_string()))?;
            if let Some(source_root) = &source_root
                && !walked.path().canonicalize()?.starts_with(source_root)
            {
                return Err(BuildError::Invalid(format!(
                    "followed symlink {} escapes source {}",
                    walked.path().display(),
                    self.source.display()
                )));
            }
            let relative = walked.path().strip_prefix(&self.source).unwrap();
            let relative_match = slash_path(relative)?;
            if (!self.include.is_empty() && !include.is_match(&relative_match))
                || exclude.is_match(&relative_match)
            {
                continue;
            }
            matched = true;
            let path = prefix.join(relative);
            let destination = join_destination(&destination, &path)?;
            insert_entry(
                entries,
                destination,
                self.entry_for(
                    walked.path(),
                    &if self.follow_symlinks {
                        fs::metadata(walked.path())?
                    } else {
                        fs::symlink_metadata(walked.path())?
                    },
                )?,
            );
        }
        if !matched && (!self.include.is_empty() || !self.exclude.is_empty()) {
            return Err(BuildError::Invalid(format!(
                "file mapping from {} matched no files",
                self.source.display()
            )));
        }
        Ok(())
    }

    fn entry_for(&self, source: &Path, metadata: &fs::Metadata) -> Result<Entry, BuildError> {
        let file_type = metadata.file_type();
        let (kind, link, default_mode) = if file_type.is_symlink() {
            (EntryType::Symlink, Some(fs::read_link(source)?), 0o777)
        } else if file_type.is_dir() {
            (EntryType::Directory, None, 0o755)
        } else if file_type.is_file() {
            (EntryType::Regular, None, host_mode(metadata, 0o644))
        } else {
            return Err(BuildError::Invalid(format!(
                "unsupported file type at {}",
                source.display()
            )));
        };
        Ok(Entry {
            source: file_type.is_file().then(|| source.to_owned()),
            data: None,
            link,
            kind,
            mode: self.mode.unwrap_or(default_mode),
            uid: self.uid,
            gid: self.gid,
            device_major: None,
            device_minor: None,
        })
    }
}

#[cfg(unix)]
fn host_mode(metadata: &fs::Metadata, fallback: u32) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    let mode = metadata.permissions().mode() & 0o7777;
    if mode == 0 { fallback } else { mode }
}

#[cfg(not(unix))]
fn host_mode(_metadata: &fs::Metadata, fallback: u32) -> u32 {
    fallback
}

pub(super) fn insert_entry(entries: &mut BTreeMap<String, Entry>, path: String, entry: Entry) {
    let child_prefix = format!("{path}/");
    entries.retain(|existing, _| {
        existing != &path && (entry.kind.is_dir() || !existing.starts_with(&child_prefix))
    });
    for ancestor in Path::new(&path)
        .ancestors()
        .skip(1)
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
    {
        let ancestor = slash_path(ancestor).unwrap();
        if entries
            .get(&ancestor)
            .is_some_and(|entry| !entry.kind.is_dir())
        {
            entries.remove(&ancestor);
        }
    }
    entries.insert(path, entry);
}

fn normalize_destination(destination: &str) -> Result<PathBuf, BuildError> {
    if !destination.starts_with('/') {
        return Err(BuildError::Invalid(format!(
            "image destination {destination:?} must be absolute"
        )));
    }
    let path = Path::new(destination);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => normalized.push(part),
            _ => {
                return Err(BuildError::Invalid(format!(
                    "image destination {destination:?} cannot contain . or .."
                )));
            }
        }
    }
    Ok(normalized)
}

fn join_destination(base: &Path, relative: &Path) -> Result<String, BuildError> {
    slash_path(&base.join(relative))
}

pub(super) fn slash_path(path: &Path) -> Result<String, BuildError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().ok_or_else(|| {
                BuildError::Invalid(format!("path {} is not valid UTF-8", path.display()))
            })?),
            Component::CurDir => {}
            _ => {
                return Err(BuildError::Invalid(format!(
                    "path {} is not portable",
                    path.display()
                )));
            }
        }
    }
    Ok(parts.join("/"))
}

fn build_globs(patterns: &[String]) -> Result<GlobSet, BuildError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(true)
            .build()
            .map_err(|error| BuildError::Invalid(format!("invalid glob {pattern:?}: {error}")))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|error| BuildError::Invalid(error.to_string()))
}

struct DigestWriter<W> {
    inner: W,
    hasher: Sha256,
    size: u64,
}

impl<W> DigestWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            size: 0,
        }
    }

    fn finish(self) -> (W, String, u64) {
        (self.inner, hex::encode(self.hasher.finalize()), self.size)
    }
}

impl<W: Write> Write for DigestWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        self.size += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
