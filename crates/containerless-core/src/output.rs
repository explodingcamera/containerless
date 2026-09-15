use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tar::{Builder, EntryType, Header};
use tempfile::{NamedTempFile, TempDir};
use tracing::{debug, info};

use crate::image::Index;
use crate::{BuildError, BuildResult, ImageSet, OciOutput};

pub(super) fn write(images: &ImageSet, output: &OciOutput) -> Result<BuildResult, BuildError> {
    if images.images.is_empty() {
        return Err(BuildError::Invalid(
            "image set must contain at least one image".to_owned(),
        ));
    }
    if images.source_date_epoch > u32::MAX.into() {
        return Err(BuildError::Invalid(
            "build timestamp is too large for a deterministic gzip header".to_owned(),
        ));
    }
    let mut image_names = BTreeSet::new();
    for image in &images.images {
        if !image_names.insert(&image.name) {
            return Err(BuildError::Invalid(format!(
                "image set contains duplicate image name {:?}",
                image.name
            )));
        }
        let mut platforms = BTreeSet::new();
        for platform in &image.platforms {
            if !platforms.insert(&platform.platform) {
                return Err(BuildError::Invalid(format!(
                    "image {:?} contains duplicate platform {}",
                    image.name, platform.platform
                )));
            }
        }
    }
    let destination = output.path();
    if destination.exists() {
        return Err(BuildError::Invalid(format!(
            "output {} already exists",
            destination.display()
        )));
    }
    let parent = output_parent(destination);
    fs::create_dir_all(parent)?;
    let output_path = parent
        .canonicalize()?
        .join(destination.file_name().ok_or_else(|| {
            BuildError::Invalid(format!("invalid output path {}", destination.display()))
        })?);
    for image in &images.images {
        for platform in &image.platforms {
            for mapping in platform.layers.iter().flat_map(|layer| &layer.files) {
                let metadata = if mapping.follow_symlinks {
                    fs::metadata(&mapping.source)
                } else {
                    fs::symlink_metadata(&mapping.source)
                }
                .map_err(|error| {
                    BuildError::Invalid(format!(
                        "cannot read {}: {error}",
                        mapping.source.display()
                    ))
                })?;
                let source = if metadata.file_type().is_symlink() {
                    let parent = output_parent(&mapping.source).canonicalize()?;
                    parent.join(mapping.source.file_name().ok_or_else(|| {
                        BuildError::Invalid(format!(
                            "invalid source path {}",
                            mapping.source.display()
                        ))
                    })?)
                } else {
                    mapping.source.canonicalize()?
                };
                if output_path == source || metadata.is_dir() && output_path.starts_with(&source) {
                    return Err(BuildError::Invalid(format!(
                        "output {} cannot be inside source {}",
                        destination.display(),
                        mapping.source.display()
                    )));
                }
            }
        }
    }
    info!(output = %destination.display(), images = images.images.len(), "building OCI image");
    let stage = TempDir::new_in(parent)?;
    let blobs = stage.path().join("blobs/sha256");
    fs::create_dir_all(&blobs)?;
    fs::write(
        stage.path().join("oci-layout"),
        b"{\"imageLayoutVersion\":\"1.0.0\"}\n",
    )?;

    let mut descriptors = Vec::new();
    let mut image_digests = BTreeMap::new();
    for image in &images.images {
        let descriptor = image.build_manifest(&blobs, images.source_date_epoch)?;
        image_digests.insert(image.name.clone(), descriptor.digest().to_owned());
        let references = if image.references.is_empty() {
            vec![image.name.clone()]
        } else {
            image.references.clone()
        };
        descriptors.extend(
            references
                .into_iter()
                .map(|reference| descriptor.clone().with_reference(reference)),
        );
    }
    descriptors.sort_by(|left, right| left.digest().cmp(right.digest()));
    let index_bytes = serde_json::to_vec(&Index::new(descriptors))?;
    fs::write(stage.path().join("index.json"), &index_bytes)?;
    let index_digest = format!("sha256:{}", hex::encode(Sha256::digest(&index_bytes)));

    match output {
        OciOutput::Layout(destination) => persist_directory(stage, destination)?,
        OciOutput::Archive(destination) => {
            persist_archive(stage.path(), destination, images.source_date_epoch)?
        }
    }
    debug!(digest = %index_digest, "wrote OCI output");
    Ok(BuildResult {
        index_digest,
        image_digests,
    })
}

pub(super) fn export_layout(
    layout: &Path,
    output: &OciOutput,
    source_date_epoch: u64,
) -> Result<(), BuildError> {
    let destination = output.path();
    if destination.exists() {
        return Err(BuildError::Invalid(format!(
            "output {} already exists",
            destination.display()
        )));
    }
    match output {
        OciOutput::Archive(destination) => persist_archive(layout, destination, source_date_epoch),
        OciOutput::Layout(destination) => {
            let parent = output_parent(destination);
            fs::create_dir_all(parent)?;
            let stage = TempDir::new_in(parent)?;
            fs::create_dir_all(stage.path().join("blobs/sha256"))?;
            fs::copy(layout.join("index.json"), stage.path().join("index.json"))?;
            fs::copy(layout.join("oci-layout"), stage.path().join("oci-layout"))?;
            for blob in fs::read_dir(layout.join("blobs/sha256"))? {
                let blob = blob?;
                fs::copy(
                    blob.path(),
                    stage.path().join("blobs/sha256").join(blob.file_name()),
                )?;
            }
            persist_directory(stage, destination)
        }
    }
}

fn persist_directory(stage: TempDir, destination: &Path) -> Result<(), BuildError> {
    let source = stage.keep();
    fs::rename(&source, destination).map_err(|error| {
        let _ = fs::remove_dir_all(source);
        BuildError::Io(error)
    })
}

fn persist_archive(stage: &Path, destination: &Path, timestamp: u64) -> Result<(), BuildError> {
    let parent = output_parent(destination);
    let temporary = NamedTempFile::new_in(parent)?;
    {
        let mut archive = Builder::new(temporary.as_file());
        append_directory(&mut archive, "blobs", timestamp)?;
        append_directory(&mut archive, "blobs/sha256", timestamp)?;
        let mut blobs = fs::read_dir(stage.join("blobs/sha256"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        blobs.sort();
        for blob in blobs {
            let name = blob.file_name().unwrap();
            append_file(
                &mut archive,
                &blob,
                &PathBuf::from("blobs/sha256").join(name),
                timestamp,
            )?;
        }
        append_file(
            &mut archive,
            &stage.join("index.json"),
            Path::new("index.json"),
            timestamp,
        )?;
        append_file(
            &mut archive,
            &stage.join("oci-layout"),
            Path::new("oci-layout"),
            timestamp,
        )?;
        archive.finish()?;
    }
    temporary
        .persist(destination)
        .map_err(|error| BuildError::Io(error.error))?;
    Ok(())
}

fn output_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn append_directory(archive: &mut Builder<&File>, path: &str, timestamp: u64) -> io::Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(timestamp);
    header.set_size(0);
    header.set_cksum();
    archive.append_data(&mut header, path, io::empty())
}

fn append_file(
    archive: &mut Builder<&File>,
    source: &Path,
    destination: &Path,
    timestamp: u64,
) -> io::Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(timestamp);
    header.set_size(fs::metadata(source)?.len());
    header.set_cksum();
    archive.append_data(&mut header, destination, File::open(source)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_current_directory_for_bare_output_names() {
        assert_eq!(output_parent(Path::new("image.tar")), Path::new("."));
        assert_eq!(output_parent(Path::new("out/image.tar")), Path::new("out"));
    }
}
