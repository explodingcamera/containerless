use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

use containerless_core::{
    FileMapping, Image, Layer, OciOutput, Platform, PlatformImage, RuntimeConfig,
};
use flate2::read::GzDecoder;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn builds_a_valid_oci_layout() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("app");
    fs::write(&source, b"hello image\n").unwrap();
    let output = temporary.path().join("layout");

    let result = image(&source)
        .build(OciOutput::Layout(output.clone()))
        .unwrap();
    assert_eq!(
        digest(&fs::read(output.join("index.json")).unwrap()),
        result.index_digest
    );
    assert_eq!(
        fs::read_to_string(output.join("oci-layout")).unwrap(),
        "{\"imageLayoutVersion\":\"1.0.0\"}\n"
    );

    let index: Value = read_json(&output.join("index.json"));
    assert_eq!(index["manifests"].as_array().unwrap().len(), 2);
    assert_eq!(
        index["manifests"][1]["annotations"]["org.opencontainers.image.ref.name"],
        "example.com/app:v1"
    );
    let descriptor = &index["manifests"][0];
    assert_eq!(
        descriptor["annotations"]["org.opencontainers.image.ref.name"],
        "app"
    );
    let manifest = read_blob_json(&output, descriptor["digest"].as_str().unwrap());
    let config = read_blob_json(&output, manifest["config"]["digest"].as_str().unwrap());
    assert_eq!(config["architecture"], "amd64");
    assert_eq!(config["os"], "linux");
    assert_eq!(config["config"]["Entrypoint"][0], "/usr/bin/app");
    assert_eq!(config["config"]["Env"][0], "MODE=production");

    let layer = &manifest["layers"][0];
    let layer_bytes = read_blob(&output, layer["digest"].as_str().unwrap());
    assert_eq!(digest(&layer_bytes), layer["digest"]);
    assert_eq!(layer_bytes.len() as u64, layer["size"]);
    let mut archive = tar::Archive::new(GzDecoder::new(layer_bytes.as_slice()));
    let mut entries = archive.entries().unwrap();
    let mut app = entries
        .find(|entry| entry.as_ref().unwrap().path().unwrap() == Path::new("usr/bin/app"))
        .unwrap()
        .unwrap();
    assert_eq!(app.header().mode().unwrap(), 0o755);
    let mut contents = String::new();
    app.read_to_string(&mut contents).unwrap();
    assert_eq!(contents, "hello image\n");
}

#[test]
fn builds_deterministic_oci_archives() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("app");
    fs::write(&source, b"deterministic\n").unwrap();
    let first = temporary.path().join("first.tar");
    let second = temporary.path().join("second.tar");

    let image = image(&source);
    image
        .clone()
        .build(OciOutput::Archive(first.clone()))
        .unwrap();
    image.build(OciOutput::Archive(second.clone())).unwrap();

    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
}

#[cfg(unix)]
#[test]
fn builds_a_directly_mapped_dangling_symlink() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("app");
    symlink("/runtime/app", &source).unwrap();
    let output = temporary.path().join("layout");

    image(&source)
        .build(OciOutput::Layout(output.clone()))
        .unwrap();

    let index: Value = read_json(&output.join("index.json"));
    let manifest = read_blob_json(&output, index["manifests"][0]["digest"].as_str().unwrap());
    let layer_bytes = read_blob(&output, manifest["layers"][0]["digest"].as_str().unwrap());
    let mut archive = tar::Archive::new(GzDecoder::new(layer_bytes.as_slice()));
    let entry = archive.entries().unwrap().next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap(), Path::new("usr/bin/app"));
    assert_eq!(
        entry.link_name().unwrap().unwrap(),
        Path::new("/runtime/app")
    );
}

fn image(source: &Path) -> Image {
    Image {
        references: vec!["app".to_owned(), "example.com/app:v1".to_owned()],
        ..Image::new(
            "app",
            vec![PlatformImage {
                layers: vec![Layer {
                    files: vec![FileMapping {
                        mode: Some(0o755),
                        ..FileMapping::new(source, "/usr/bin/app")
                    }],
                }],
                runtime_config: RuntimeConfig {
                    entrypoint: Some(vec!["/usr/bin/app".to_owned()]),
                    env: BTreeMap::from([("MODE".to_owned(), "production".to_owned())]),
                    ..RuntimeConfig::default()
                },
                ..PlatformImage::scratch(Platform::from_str("linux/amd64").unwrap())
            }],
        )
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn read_blob_json(layout: &Path, digest: &str) -> Value {
    serde_json::from_slice(&read_blob(layout, digest)).unwrap()
}

fn read_blob(layout: &Path, digest: &str) -> Vec<u8> {
    fs::read(
        layout
            .join("blobs/sha256")
            .join(digest.strip_prefix("sha256:").unwrap()),
    )
    .unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}
