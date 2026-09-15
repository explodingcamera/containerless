use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use docker_credential::{CredentialRetrievalError, DockerCredential};
use oci_client::client::{Client, ClientConfig, ClientProtocol, ImageLayer};
use oci_client::manifest::{
    IMAGE_DOCKER_LAYER_GZIP_MEDIA_TYPE, IMAGE_DOCKER_LAYER_TAR_MEDIA_TYPE,
    IMAGE_LAYER_GZIP_MEDIA_TYPE, IMAGE_LAYER_MEDIA_TYPE,
    IMAGE_LAYER_NONDISTRIBUTABLE_GZIP_MEDIA_TYPE, IMAGE_LAYER_NONDISTRIBUTABLE_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE, OciDescriptor,
};
use oci_client::secrets::RegistryAuth;
use oci_client::{Reference, RegistryOperation};
use serde_json::Value;
use tracing::{debug, info};

use crate::{
    BaseImage, BaseLayer, BuildResult, ImageSet, OciOutput, Platform, PublishError, RegistryError,
};

/// Access to OCI registries using Docker credentials when available.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    plain_http: Vec<String>,
}

impl Registry {
    /// Creates a registry client that permits plain HTTP for the listed hosts.
    pub fn new(plain_http: Vec<String>) -> Self {
        Self { plain_http }
    }

    /// Pulls and verifies the selected platform of an image for use as a base.
    pub async fn pull(&self, image: &str, platform: &Platform) -> Result<BaseImage, RegistryError> {
        let reference: Reference = image
            .parse()
            .map_err(|error| RegistryError(format!("invalid base image {image:?}: {error}")))?;
        let target = platform.clone();
        let client = Client::try_from(ClientConfig {
            protocol: self.protocol(&reference),
            platform_resolver: Some(Box::new(move |entries| {
                entries
                    .iter()
                    .find(|entry| {
                        entry.platform.as_ref().is_some_and(|candidate| {
                            candidate.os.to_string() == target.os()
                                && candidate.architecture.to_string() == target.architecture()
                                && target.variant().is_none_or(|variant| {
                                    candidate.variant.as_deref() == Some(variant)
                                })
                        })
                    })
                    .map(|entry| entry.digest.clone())
            })),
            ..ClientConfig::default()
        })
        .map_err(|error| RegistryError(format!("cannot create registry client: {error}")))?;

        info!(base = %image, platform = %platform, "pulling base image");
        let pulled = client
            .pull(
                &reference,
                &Self::auth(&reference)?,
                vec![
                    IMAGE_LAYER_MEDIA_TYPE,
                    IMAGE_LAYER_GZIP_MEDIA_TYPE,
                    IMAGE_DOCKER_LAYER_TAR_MEDIA_TYPE,
                    IMAGE_DOCKER_LAYER_GZIP_MEDIA_TYPE,
                    IMAGE_LAYER_NONDISTRIBUTABLE_MEDIA_TYPE,
                    IMAGE_LAYER_NONDISTRIBUTABLE_GZIP_MEDIA_TYPE,
                    "application/vnd.oci.image.layer.v1.tar+zstd",
                    "application/vnd.oci.image.layer.nondistributable.v1.tar+zstd",
                    "application/vnd.docker.image.rootfs.foreign.diff.tar.gzip",
                ],
            )
            .await
            .map_err(|error| RegistryError(format!("cannot pull base image {image:?}: {error}")))?;
        let manifest = pulled.manifest.ok_or_else(|| {
            RegistryError(format!("base image {image:?} returned no image manifest"))
        })?;
        let configuration: Value =
            serde_json::from_slice(&pulled.config.data).map_err(|error| {
                RegistryError(format!(
                    "base image {image:?} has invalid config JSON: {error}"
                ))
            })?;
        if configuration.get("os").and_then(Value::as_str) != Some(platform.os())
            || configuration.get("architecture").and_then(Value::as_str)
                != Some(platform.architecture())
        {
            return Err(RegistryError(format!(
                "base image {image:?} does not match platform {}/{}",
                platform.os(),
                platform.architecture()
            )));
        }

        let layers = Self::resolve_layers(manifest.layers, pulled.layers)?;
        debug!(base = %image, layers = layers.len(), "pulled base image");
        Ok(BaseImage {
            manifest_media_type: manifest
                .media_type
                .unwrap_or_else(|| OCI_IMAGE_MEDIA_TYPE.to_owned()),
            config_media_type: pulled.config.media_type,
            config: configuration,
            layers,
            annotations: manifest.annotations.unwrap_or_default(),
        })
    }

    /// Builds and publishes an image set, optionally retaining a local output.
    pub async fn publish(
        &self,
        images: ImageSet,
        output: Option<OciOutput>,
    ) -> Result<BuildResult, PublishError> {
        let publications = images
            .images
            .iter()
            .flat_map(|image| {
                image
                    .references
                    .iter()
                    .map(|reference| (image.name.clone(), reference.clone()))
            })
            .collect::<Vec<_>>();
        if publications.is_empty() {
            return Err(PublishError::Invalid(
                "image set has no references to publish".to_owned(),
            ));
        }

        let source_date_epoch = images.source_date_epoch;
        let temporary = tempfile::tempdir().map_err(crate::BuildError::from)?;
        let layout = temporary.path().join("layout");
        let result = images.build(OciOutput::Layout(layout.clone()))?;
        let publications = publications
            .into_iter()
            .map(|(image, reference)| (reference, result.image_digests[&image].clone()))
            .collect::<Vec<_>>();
        self.publish_layout(&layout, &publications).await?;
        if let Some(output) = output {
            output.export_layout(&layout, source_date_epoch)?;
        }
        Ok(result)
    }

    async fn publish_layout(
        &self,
        layout: &Path,
        publications: &[(String, String)],
    ) -> Result<(), RegistryError> {
        for (reference, digest) in publications {
            let reference: Reference = reference.parse().map_err(|error| {
                RegistryError(format!("invalid image reference {reference:?}: {error}"))
            })?;
            let client = Client::try_from(ClientConfig {
                protocol: self.protocol(&reference),
                ..ClientConfig::default()
            })
            .map_err(|error| RegistryError(format!("cannot create registry client: {error}")))?;
            let auth = Self::auth(&reference)?;
            client
                .auth(&reference, &auth, RegistryOperation::Push)
                .await
                .map_err(|error| {
                    RegistryError(format!(
                        "cannot authenticate to {}: {error}",
                        reference.registry()
                    ))
                })?;

            info!(image = %reference, "publishing image");
            let bytes = Self::read_blob(layout, digest)?;
            let manifest: Value = serde_json::from_slice(&bytes).map_err(|error| {
                RegistryError(format!("invalid image manifest {digest}: {error}"))
            })?;
            let media_type = manifest["mediaType"]
                .as_str()
                .ok_or_else(|| RegistryError(format!("manifest {digest} has no mediaType")))?;
            if media_type.ends_with("image.index.v1+json") {
                for child in manifest["manifests"].as_array().ok_or_else(|| {
                    RegistryError(format!("image index {digest} has no manifests"))
                })? {
                    let child_digest = child["digest"].as_str().ok_or_else(|| {
                        RegistryError(format!("image index {digest} has an invalid descriptor"))
                    })?;
                    Self::push_image_manifest(
                        &client,
                        &reference,
                        &reference.clone_with_digest(child_digest.to_owned()),
                        layout,
                        child_digest,
                    )
                    .await?;
                }
            } else {
                Self::push_manifest_blobs(&client, &reference, layout, &manifest).await?;
            }
            client
                .push_manifest_raw(
                    &reference,
                    bytes,
                    media_type.parse().map_err(|error| {
                        RegistryError(format!("invalid manifest media type: {error}"))
                    })?,
                )
                .await
                .map_err(|error| RegistryError(format!("cannot publish {reference}: {error}")))?;
        }
        Ok(())
    }

    fn protocol(&self, reference: &Reference) -> ClientProtocol {
        ClientProtocol::HttpsExcept(
            self.plain_http
                .iter()
                .map(|registry| {
                    if registry == reference.registry() {
                        reference.resolve_registry().to_owned()
                    } else {
                        registry.clone()
                    }
                })
                .collect(),
        )
    }

    fn auth(reference: &Reference) -> Result<RegistryAuth, RegistryError> {
        let server = if reference.registry() == "docker.io" {
            "https://index.docker.io/v1/"
        } else {
            reference.resolve_registry()
        };
        match docker_credential::get_credential(server) {
            Ok(DockerCredential::UsernamePassword(username, password)) => {
                Ok(RegistryAuth::Basic(username, password))
            }
            Ok(DockerCredential::IdentityToken(token)) => Ok(RegistryAuth::Bearer(token)),
            Err(
                CredentialRetrievalError::ConfigNotFound
                | CredentialRetrievalError::ConfigReadError
                | CredentialRetrievalError::NoCredentialConfigured,
            ) => Ok(RegistryAuth::Anonymous),
            Err(error) => Err(RegistryError(format!(
                "cannot read credentials for {server}: {error}"
            ))),
        }
    }

    fn resolve_layers(
        descriptors: Vec<OciDescriptor>,
        pulled_layers: Vec<ImageLayer>,
    ) -> Result<Vec<BaseLayer>, RegistryError> {
        let data_by_digest = pulled_layers
            .into_iter()
            .map(|layer| (layer.sha256_digest(), layer.data.to_vec()))
            .collect::<BTreeMap<_, _>>();
        let mut layers = Vec::new();
        for descriptor in descriptors {
            if !descriptor.digest.starts_with("sha256:") {
                return Err(RegistryError(format!(
                    "base layer {} uses an unsupported digest algorithm",
                    descriptor.digest
                )));
            }
            let data = data_by_digest
                .get(&descriptor.digest)
                .cloned()
                .ok_or_else(|| {
                    RegistryError(format!(
                        "base image response omitted layer {}",
                        descriptor.digest
                    ))
                })?;
            let expected_size: u64 = descriptor
                .size
                .try_into()
                .map_err(|_| RegistryError("base layer has a negative size".to_owned()))?;
            let layer = BaseLayer::new(descriptor.media_type, data)
                .with_annotations(descriptor.annotations.unwrap_or_default());
            if layer.digest() != descriptor.digest || layer.size() != expected_size {
                return Err(RegistryError(format!(
                    "base layer {} does not match its descriptor",
                    descriptor.digest
                )));
            }
            layers.push(layer);
        }
        Ok(layers)
    }

    async fn push_image_manifest(
        client: &Client,
        repository: &Reference,
        target: &Reference,
        layout: &Path,
        digest: &str,
    ) -> Result<(), RegistryError> {
        let bytes = Self::read_blob(layout, digest)?;
        let manifest: Value = serde_json::from_slice(&bytes)
            .map_err(|error| RegistryError(format!("invalid image manifest {digest}: {error}")))?;
        Self::push_manifest_blobs(client, repository, layout, &manifest).await?;
        let media_type = manifest["mediaType"]
            .as_str()
            .ok_or_else(|| RegistryError(format!("manifest {digest} has no mediaType")))?;
        client
            .push_manifest_raw(
                target,
                bytes,
                media_type.parse().map_err(|error| {
                    RegistryError(format!("invalid manifest media type: {error}"))
                })?,
            )
            .await
            .map_err(|error| RegistryError(format!("cannot publish manifest {digest}: {error}")))?;
        Ok(())
    }

    async fn push_manifest_blobs(
        client: &Client,
        reference: &Reference,
        layout: &Path,
        manifest: &Value,
    ) -> Result<(), RegistryError> {
        let descriptors = manifest.get("config").into_iter().chain(
            manifest["layers"]
                .as_array()
                .ok_or_else(|| RegistryError("image manifest has no layers".to_owned()))?,
        );
        for descriptor in descriptors {
            let digest = descriptor["digest"].as_str().ok_or_else(|| {
                RegistryError("image manifest has an invalid descriptor".to_owned())
            })?;
            client
                .push_blob(reference, Self::read_blob(layout, digest)?, digest)
                .await
                .map_err(|error| RegistryError(format!("cannot push blob {digest}: {error}")))?;
        }
        Ok(())
    }

    fn read_blob(layout: &Path, digest: &str) -> Result<Vec<u8>, RegistryError> {
        let digest = digest
            .strip_prefix("sha256:")
            .ok_or_else(|| RegistryError(format!("unsupported digest {digest:?}")))?;
        fs::read(layout.join("blobs/sha256").join(digest))
            .map_err(|error| RegistryError(format!("cannot read blob sha256:{digest}: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_repeated_layer_descriptors() {
        let pulled = ImageLayer::oci_v1(b"layer".to_vec(), None);
        let descriptor = OciDescriptor {
            media_type: IMAGE_LAYER_MEDIA_TYPE.to_owned(),
            digest: pulled.sha256_digest(),
            size: pulled.data.len() as i64,
            urls: None,
            annotations: None,
            artifact_type: None,
        };

        let layers =
            Registry::resolve_layers(vec![descriptor.clone(), descriptor], vec![pulled]).unwrap();

        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].digest(), layers[1].digest());
    }
}
