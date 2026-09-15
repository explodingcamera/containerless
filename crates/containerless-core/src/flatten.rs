use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::layer::{Entry, LayerBlob, insert_entry, slash_path, write_layer};
use crate::{BaseLayer, BuildError, Layer};

pub(super) fn build_flattened_layer(
    base_layers: &[BaseLayer],
    local_layers: &[Layer],
    blobs: &Path,
    timestamp: u64,
) -> Result<LayerBlob, BuildError> {
    let mut entries = BTreeMap::new();
    for layer in base_layers {
        apply_base_layer(layer, &mut entries)?;
    }
    for layer in local_layers {
        for mapping in &layer.files {
            mapping.collect_into(&mut entries)?;
        }
    }
    write_layer(entries, blobs, timestamp)
}

fn apply_base_layer(
    layer: &BaseLayer,
    entries: &mut BTreeMap<String, Entry>,
) -> Result<(), BuildError> {
    let compressed: Box<dyn Read> =
        if layer.media_type().ends_with("+gzip") || layer.media_type().ends_with(".gzip") {
            Box::new(flate2::read::GzDecoder::new(layer.bytes()))
        } else if layer.media_type().ends_with("+zstd") || layer.media_type().ends_with(".zstd") {
            Box::new(zstd::Decoder::new(layer.bytes())?)
        } else if layer.media_type().ends_with(".tar") {
            Box::new(Cursor::new(layer.bytes()))
        } else {
            return Err(BuildError::Unsupported(format!(
                "cannot flatten layer media type {:?}",
                layer.media_type()
            )));
        };
    let mut archive = tar::Archive::new(compressed);
    let mut additions = BTreeMap::new();
    let mut whiteouts = Vec::new();
    for archived in archive.entries()? {
        let mut archived = archived?;
        let path = slash_path(&archived.path()?)?;
        if path.is_empty() {
            continue;
        }
        let path = Path::new(&path);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name == ".wh..wh..opq" {
            let parent = path
                .parent()
                .map(slash_path)
                .transpose()?
                .unwrap_or_default();
            whiteouts.push((parent, true));
            continue;
        }
        if let Some(removed) = name.strip_prefix(".wh.") {
            let removed = path.parent().unwrap_or_else(|| Path::new("")).join(removed);
            whiteouts.push((slash_path(&removed)?, false));
            continue;
        }

        let kind = archived.header().entry_type();
        if !(kind.is_file()
            || kind.is_dir()
            || kind.is_symlink()
            || kind.is_hard_link()
            || kind.is_character_special()
            || kind.is_block_special()
            || kind.is_fifo())
        {
            return Err(BuildError::Unsupported(format!(
                "cannot flatten archive entry type at {}",
                path.display()
            )));
        }
        let link = if kind.is_symlink() || kind.is_hard_link() {
            Some(
                archived
                    .link_name()?
                    .ok_or_else(|| {
                        BuildError::Invalid(format!("missing link target for {path:?}"))
                    })?
                    .into_owned(),
            )
        } else {
            None
        };
        let mode = archived.header().mode()?;
        let uid = archived.header().uid()?;
        let gid = archived.header().gid()?;
        let (device_major, device_minor) = if kind.is_character_special() || kind.is_block_special()
        {
            (
                archived.header().device_major()?,
                archived.header().device_minor()?,
            )
        } else {
            (None, None)
        };
        let data = if kind.is_file() {
            let mut data = Vec::new();
            archived.read_to_end(&mut data)?;
            Some(data)
        } else {
            None
        };
        insert_entry(
            &mut additions,
            slash_path(path)?,
            Entry {
                source: None,
                data,
                link,
                kind,
                mode,
                uid,
                gid,
                device_major,
                device_minor,
            },
        );
    }
    for (removed, opaque) in whiteouts {
        let prefix = if removed.is_empty() {
            String::new()
        } else {
            format!("{removed}/")
        };
        entries.retain(|existing, _| {
            if opaque {
                !existing.starts_with(&prefix)
            } else {
                existing != &removed && !existing.starts_with(&prefix)
            }
        });
    }
    for (path, entry) in additions {
        insert_entry(entries, path, entry);
    }
    let hard_links = entries
        .iter()
        .filter(|(_, entry)| entry.kind.is_hard_link())
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let snapshot = entries.clone();
    for path in hard_links {
        let mut resolving = Vec::new();
        let entry = materialize_hard_link(&path, &snapshot, &mut resolving)?;
        entries.insert(path, entry);
    }
    Ok(())
}

fn materialize_hard_link(
    path: &str,
    entries: &BTreeMap<String, Entry>,
    resolving: &mut Vec<String>,
) -> Result<Entry, BuildError> {
    if resolving.iter().any(|existing| existing == path) {
        return Err(BuildError::Invalid(format!(
            "hard-link cycle includes {path:?}"
        )));
    }
    let entry = entries
        .get(path)
        .ok_or_else(|| BuildError::Invalid(format!("hard-link target {path:?} does not exist")))?;
    if !entry.kind.is_hard_link() {
        if !entry.kind.is_file() {
            return Err(BuildError::Invalid(format!(
                "hard-link target {path:?} is not a file"
            )));
        }
        return Ok(entry.clone());
    }

    resolving.push(path.to_owned());
    let target = slash_path(
        entry
            .link
            .as_deref()
            .ok_or_else(|| BuildError::Invalid(format!("hard link {path:?} has no target")))?,
    )?;
    let mut target = materialize_hard_link(&target, entries, resolving)?;
    resolving.pop();
    target.source = None;
    target.link = None;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;

    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::{Builder, EntryType, Header};

    use super::*;

    #[test]
    fn preserves_hard_link_contents_when_target_is_removed() {
        let temporary = tempfile::tempdir().unwrap();
        let blobs = temporary.path().join("blobs");
        fs::create_dir(&blobs).unwrap();
        let base_layers = vec![
            base_layer(|archive| {
                append_file(archive, "target", b"contents");
                let mut header = Header::new_gnu();
                header.set_entry_type(EntryType::Link);
                set_metadata(&mut header);
                header.set_size(0);
                header.set_cksum();
                archive.append_link(&mut header, "link", "target").unwrap();
            }),
            base_layer(|archive| append_file(archive, ".wh.target", b"")),
        ];

        let blob = build_flattened_layer(&base_layers, &[], &blobs, 0).unwrap();
        let compressed =
            fs::read(blobs.join(blob.digest.strip_prefix("sha256:").unwrap())).unwrap();
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(compressed.as_slice()));
        let mut link = archive
            .entries()
            .unwrap()
            .find(|entry| entry.as_ref().unwrap().path().unwrap() == Path::new("link"))
            .unwrap()
            .unwrap();
        assert!(link.header().entry_type().is_file());
        let mut contents = String::new();
        link.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "contents");
    }

    #[test]
    fn reads_docker_gzip_layers() {
        let layer = base_layer(|archive| append_file(archive, "file", b"contents"));
        let layer = BaseLayer::new(
            "application/vnd.docker.image.rootfs.diff.tar.gzip",
            layer.bytes().to_vec(),
        );

        let mut entries = BTreeMap::new();
        apply_base_layer(&layer, &mut entries).unwrap();

        assert_eq!(
            entries["file"].data.as_deref(),
            Some(b"contents".as_slice())
        );
    }

    #[test]
    fn reads_oci_zstd_layers() {
        let mut archive = Builder::new(Vec::new());
        append_file(&mut archive, "file", b"contents");
        archive.finish().unwrap();
        let tar = archive.into_inner().unwrap();
        let layer = BaseLayer::new(
            "application/vnd.oci.image.layer.v1.tar+zstd",
            zstd::encode_all(tar.as_slice(), 0).unwrap(),
        );

        let mut entries = BTreeMap::new();
        apply_base_layer(&layer, &mut entries).unwrap();

        assert_eq!(
            entries["file"].data.as_deref(),
            Some(b"contents".as_slice())
        );
    }

    fn base_layer(write: impl FnOnce(&mut Builder<GzEncoder<Vec<u8>>>)) -> BaseLayer {
        let mut archive = Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        write(&mut archive);
        archive.finish().unwrap();
        let data = archive.into_inner().unwrap().finish().unwrap();
        BaseLayer::new("application/vnd.oci.image.layer.v1.tar+gzip", data)
    }

    fn append_file<W: std::io::Write>(archive: &mut Builder<W>, path: &str, data: &[u8]) {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        set_metadata(&mut header);
        header.set_size(data.len() as u64);
        header.set_cksum();
        archive.append_data(&mut header, path, data).unwrap();
    }

    fn set_metadata(header: &mut Header) {
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
    }
}
