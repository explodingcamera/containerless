use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::flatten::build_flattened_layer;
use crate::{BuildError, Image, PlatformImage, RuntimeConfig};

const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const OCI_LAYER: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
const DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
const DOCKER_LAYER: &str = "application/vnd.docker.image.rootfs.diff.tar.gzip";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Descriptor {
    media_type: String,
    digest: String,
    size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    platform: Option<OciPlatform>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    annotations: BTreeMap<String, String>,
}

impl Descriptor {
    pub(super) fn digest(&self) -> &str {
        &self.digest
    }

    pub(super) fn with_reference(mut self, reference: String) -> Self {
        self.annotations
            .insert("org.opencontainers.image.ref.name".to_owned(), reference);
        self
    }
}

#[derive(Clone, Serialize)]
struct OciPlatform {
    architecture: String,
    os: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    variant: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u8,
    media_type: String,
    config: Descriptor,
    layers: Vec<Descriptor>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    annotations: BTreeMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Index {
    schema_version: u8,
    media_type: &'static str,
    manifests: Vec<Descriptor>,
}

impl Index {
    pub(super) fn new(manifests: Vec<Descriptor>) -> Self {
        Self {
            schema_version: 2,
            media_type: INDEX_MEDIA_TYPE,
            manifests,
        }
    }
}

impl Image {
    pub(super) fn build_manifest(
        &self,
        blobs: &Path,
        timestamp: u64,
    ) -> Result<Descriptor, BuildError> {
        if self.platforms.is_empty() {
            return Err(BuildError::Invalid(format!(
                "image {:?} has no target platforms",
                self.name
            )));
        }

        let mut manifests = Vec::new();
        for platform in &self.platforms {
            info!(image = %self.name, os = %platform.platform.os(), architecture = %platform.platform.architecture(), "building platform image");
            manifests.push(platform.build_manifest(blobs, timestamp)?);
        }
        manifests.sort_by(|left, right| {
            let left = left.platform.as_ref().unwrap();
            let right = right.platform.as_ref().unwrap();
            (&left.os, &left.architecture, &left.variant).cmp(&(
                &right.os,
                &right.architecture,
                &right.variant,
            ))
        });

        if manifests.len() == 1 {
            return Ok(manifests.remove(0));
        }

        let bytes = serde_json::to_vec(&Index::new(manifests))?;
        let (digest, size) = write_blob(blobs, &bytes)?;
        debug!(image = %self.name, digest = %digest, size, "wrote image index");
        Ok(Descriptor {
            media_type: INDEX_MEDIA_TYPE.to_owned(),
            digest,
            size,
            platform: None,
            annotations: BTreeMap::new(),
        })
    }
}

impl PlatformImage {
    fn build_manifest(&self, blobs: &Path, timestamp: u64) -> Result<Descriptor, BuildError> {
        let (manifest_media_type, config_media_type, layer_media_type, mut configuration) =
            if let Some(base) = &self.base {
                let layer_media_type = if base.manifest_media_type == DOCKER_MANIFEST {
                    DOCKER_LAYER
                } else if base.manifest_media_type == OCI_MANIFEST {
                    OCI_LAYER
                } else {
                    return Err(BuildError::Unsupported(format!(
                        "unsupported base manifest media type {:?}",
                        base.manifest_media_type
                    )));
                };
                (
                    base.manifest_media_type.clone(),
                    base.config_media_type.clone(),
                    layer_media_type,
                    base.config.clone(),
                )
            } else {
                (
                    OCI_MANIFEST.to_owned(),
                    OCI_CONFIG.to_owned(),
                    OCI_LAYER,
                    json!({
                        "architecture": self.platform.architecture(),
                        "os": self.platform.os(),
                        "rootfs": { "type": "layers", "diff_ids": [] },
                        "config": {}
                    }),
                )
            };

        let configuration_object = configuration.as_object_mut().ok_or_else(|| {
            BuildError::Invalid("base image configuration must be a JSON object".to_owned())
        })?;
        let rootfs = configuration_object
            .entry("rootfs")
            .or_insert_with(|| json!({ "type": "layers", "diff_ids": [] }))
            .as_object_mut()
            .ok_or_else(|| BuildError::Invalid("base image rootfs must be an object".to_owned()))?;
        if rootfs.get("type").and_then(Value::as_str) != Some("layers") {
            return Err(BuildError::Invalid(
                "base image rootfs type must be layers".to_owned(),
            ));
        }
        let diff_ids = rootfs
            .entry("diff_ids")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| {
                BuildError::Invalid("base image diff_ids must be an array".to_owned())
            })?;

        let mut layers = Vec::new();
        if self.flatten {
            let base_layers = self
                .base
                .as_ref()
                .map(|base| base.layers.as_slice())
                .unwrap_or_default();
            let blob = build_flattened_layer(base_layers, &self.layers, blobs, timestamp)?;
            diff_ids.clear();
            diff_ids.push(Value::String(blob.diff_id));
            layers.push(Descriptor {
                media_type: layer_media_type.to_owned(),
                digest: blob.digest,
                size: blob.size,
                platform: None,
                annotations: BTreeMap::new(),
            });
            configuration_object.insert(
                "history".to_owned(),
                json!([{ "created_by": "containerless --flatten" }]),
            );
        } else {
            if let Some(base) = &self.base {
                for layer in &base.layers {
                    let (digest, size) = write_blob(blobs, layer.bytes())?;
                    layers.push(Descriptor {
                        media_type: layer.media_type().to_owned(),
                        digest,
                        size,
                        platform: None,
                        annotations: layer.annotations().clone(),
                    });
                }
            }
            let mut history = Vec::new();
            for layer in &self.layers {
                let blob = layer.build(blobs, timestamp)?;
                diff_ids.push(Value::String(blob.diff_id));
                history.push(json!({ "created_by": "containerless" }));
                layers.push(Descriptor {
                    media_type: layer_media_type.to_owned(),
                    digest: blob.digest,
                    size: blob.size,
                    platform: None,
                    annotations: BTreeMap::new(),
                });
            }
            if !history.is_empty() {
                configuration_object
                    .entry("history")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .ok_or_else(|| {
                        BuildError::Invalid("base image history must be an array".to_owned())
                    })?
                    .extend(history);
            }
        }
        self.runtime_config.apply_to(configuration_object)?;

        let config_bytes = serde_json::to_vec(&configuration)?;
        let (config_digest, config_size) = write_blob(blobs, &config_bytes)?;
        let config = Descriptor {
            media_type: config_media_type,
            digest: config_digest,
            size: config_size,
            platform: None,
            annotations: BTreeMap::new(),
        };
        let mut annotations = self
            .base
            .as_ref()
            .map(|base| base.annotations.clone())
            .unwrap_or_default();
        annotations.extend(self.annotations.clone());
        let manifest_bytes = serde_json::to_vec(&Manifest {
            schema_version: 2,
            media_type: manifest_media_type.clone(),
            config,
            layers,
            annotations,
        })?;
        let (digest, size) = write_blob(blobs, &manifest_bytes)?;
        Ok(Descriptor {
            media_type: manifest_media_type,
            digest,
            size,
            platform: Some(OciPlatform {
                architecture: self.platform.architecture().to_owned(),
                os: self.platform.os().to_owned(),
                variant: self.platform.variant().map(str::to_owned),
            }),
            annotations: BTreeMap::new(),
        })
    }
}

impl RuntimeConfig {
    fn apply_to(&self, configuration: &mut Map<String, Value>) -> Result<(), BuildError> {
        let runtime = configuration
            .entry("config")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| BuildError::Invalid("base image config must be an object".to_owned()))?;

        for (name, value) in [
            (
                "Entrypoint",
                self.entrypoint.as_ref().map(|value| json!(value)),
            ),
            ("Cmd", self.command.as_ref().map(|value| json!(value))),
            ("User", self.user.as_ref().map(|value| json!(value))),
            (
                "WorkingDir",
                self.workdir.as_ref().map(|value| json!(value)),
            ),
            (
                "StopSignal",
                self.stop_signal.as_ref().map(|value| json!(value)),
            ),
        ] {
            if let Some(value) = value {
                runtime.insert(name.to_owned(), value);
            }
        }
        if let Some(ports) = &self.expose {
            runtime.insert(
                "ExposedPorts".to_owned(),
                Value::Object(ports.iter().map(|port| (port.clone(), json!({}))).collect()),
            );
        }
        if let Some(volumes) = &self.volumes {
            runtime.insert(
                "Volumes".to_owned(),
                Value::Object(
                    volumes
                        .iter()
                        .map(|volume| (volume.clone(), json!({})))
                        .collect(),
                ),
            );
        }

        if !self.env.is_empty() {
            let mut environment = runtime
                .get("Env")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter_map(|value| value.split_once('='))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect::<BTreeMap<_, _>>();
            environment.extend(self.env.clone());
            runtime.insert(
                "Env".to_owned(),
                json!(
                    environment
                        .into_iter()
                        .map(|(name, value)| format!("{name}={value}"))
                        .collect::<Vec<_>>()
                ),
            );
        }

        if !self.labels.is_empty() {
            if runtime.get("Labels").is_none_or(Value::is_null) {
                runtime.insert("Labels".to_owned(), json!({}));
            }
            let labels = runtime
                .get_mut("Labels")
                .unwrap()
                .as_object_mut()
                .ok_or_else(|| {
                    BuildError::Invalid("base image labels must be an object".to_owned())
                })?;
            labels.extend(
                self.labels
                    .iter()
                    .map(|(name, value)| (name.clone(), Value::String(value.clone()))),
            );
        }
        Ok(())
    }
}

pub(super) fn write_blob(blobs: &Path, bytes: &[u8]) -> Result<(String, u64), BuildError> {
    let digest = hex::encode(Sha256::digest(bytes));
    let path = blobs.join(&digest);
    if !path.exists() {
        fs::write(path, bytes)?;
    }
    Ok((format!("sha256:{digest}"), bytes.len() as u64))
}
