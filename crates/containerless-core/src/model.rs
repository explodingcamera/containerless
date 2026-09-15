use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::Serialize;

use crate::{BuildError, PlatformParseError};

/// A collection of images built together as one OCI output.
#[derive(Debug, Clone, Serialize)]
pub struct ImageSet {
    pub(crate) images: Vec<Image>,
    pub(crate) source_date_epoch: u64,
}

impl ImageSet {
    /// Creates an image set with deterministic timestamp zero.
    pub fn new(images: Vec<Image>) -> Self {
        Self {
            images,
            source_date_epoch: 0,
        }
    }

    /// Sets the Unix timestamp applied to generated archive entries.
    pub fn with_source_date_epoch(mut self, source_date_epoch: u64) -> Self {
        self.source_date_epoch = source_date_epoch;
        self
    }

    /// Returns the images in this set.
    pub fn images(&self) -> &[Image] {
        &self.images
    }

    /// Returns the number of images in this set.
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Returns whether this set contains no images.
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Builds the images as an OCI layout or archive.
    pub fn build(self, output: OciOutput) -> Result<BuildResult, BuildError> {
        crate::output::write(&self, &output)
    }
}

/// A named image for one or more target platforms.
#[derive(Debug, Clone, Serialize)]
pub struct Image {
    /// Name used to select the image in an OCI layout.
    pub name: String,
    /// References attached to the image in the OCI layout.
    pub references: Vec<String>,
    /// Platform-specific image definitions.
    pub platforms: Vec<PlatformImage>,
}

impl Image {
    /// Creates an image without attached references.
    pub fn new(name: impl Into<String>, platforms: Vec<PlatformImage>) -> Self {
        Self {
            name: name.into(),
            references: Vec::new(),
            platforms,
        }
    }

    /// Builds this image as an OCI layout or archive.
    pub fn build(self, output: OciOutput) -> Result<BuildResult, BuildError> {
        ImageSet::new(vec![self]).build(output)
    }
}

/// Image contents and runtime settings for one target platform.
#[derive(Debug, Clone, Serialize)]
pub struct PlatformImage {
    /// OCI operating system and architecture.
    pub platform: Platform,
    /// Base image, or `None` for scratch.
    pub base: Option<BaseImage>,
    /// Filesystem layers in base-to-child order.
    pub layers: Vec<Layer>,
    /// Runtime configuration embedded in the image.
    pub runtime_config: RuntimeConfig,
    /// OCI manifest annotations.
    pub annotations: BTreeMap<String, String>,
    /// Emit the complete resulting filesystem as one layer.
    pub flatten: bool,
}

impl PlatformImage {
    /// Creates an empty scratch image for a platform.
    pub fn scratch(platform: Platform) -> Self {
        Self {
            platform,
            base: None,
            layers: Vec::new(),
            runtime_config: RuntimeConfig::default(),
            annotations: BTreeMap::new(),
            flatten: false,
        }
    }
}

/// An OCI target platform.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Platform {
    os: String,
    architecture: String,
    variant: Option<String>,
}

impl Platform {
    /// Creates a platform without an architecture variant.
    pub fn new(os: impl Into<String>, architecture: impl Into<String>) -> Self {
        Self {
            os: os.into(),
            architecture: architecture.into(),
            variant: None,
        }
    }

    /// Sets the architecture variant, such as `v7`.
    pub fn with_variant(mut self, variant: impl Into<String>) -> Self {
        self.variant = Some(variant.into());
        self
    }

    /// Returns Linux on the current process architecture.
    pub fn linux_for_host() -> Self {
        let architecture = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "x86" => "386",
            "aarch64" => "arm64",
            "powerpc64" if cfg!(target_endian = "little") => "ppc64le",
            "powerpc64" => "ppc64",
            architecture => architecture,
        };
        Self::new("linux", architecture)
    }

    /// Returns the operating system, such as `linux`.
    pub fn os(&self) -> &str {
        &self.os
    }

    /// Returns the architecture, such as `amd64` or `arm64`.
    pub fn architecture(&self) -> &str {
        &self.architecture
    }

    /// Returns the optional architecture variant, such as `v7`.
    pub fn variant(&self) -> Option<&str> {
        self.variant.as_deref()
    }
}

impl FromStr for Platform {
    type Err = PlatformParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts = value.split('/').collect::<Vec<_>>();
        if !(parts.len() == 2 || parts.len() == 3) || parts.iter().any(|part| part.is_empty()) {
            return Err(PlatformParseError {
                value: value.to_owned(),
            });
        }
        let platform = Self::new(parts[0], parts[1]);
        Ok(match parts.get(2) {
            Some(variant) => platform.with_variant(*variant),
            None => platform,
        })
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.os, self.architecture)?;
        if let Some(variant) = &self.variant {
            write!(formatter, "/{variant}")?;
        }
        Ok(())
    }
}

/// A filesystem layer assembled from ordered local file mappings.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Layer {
    /// Mappings applied in order. Later mappings replace earlier paths.
    pub files: Vec<FileMapping>,
}

/// An image used as the base for one target platform.
#[derive(Debug, Clone, Serialize)]
pub struct BaseImage {
    /// Media type used for the child manifest.
    pub manifest_media_type: String,
    /// Media type used for the image configuration.
    pub config_media_type: String,
    /// Original image configuration, including unknown extension fields.
    pub config: serde_json::Value,
    /// Existing compressed filesystem layers.
    pub layers: Vec<BaseLayer>,
    /// Annotations inherited from the selected base manifest.
    pub annotations: BTreeMap<String, String>,
}

/// A compressed layer and descriptor inherited from a base image.
#[derive(Debug, Clone, Serialize)]
pub struct BaseLayer {
    /// Layer media type.
    media_type: String,
    /// Content digest.
    digest: String,
    /// Descriptor size.
    size: u64,
    /// Exact compressed blob bytes.
    #[serde(skip)]
    data: Vec<u8>,
    /// Descriptor annotations.
    annotations: BTreeMap<String, String>,
}

impl BaseLayer {
    /// Creates a base layer descriptor from its exact compressed blob bytes.
    pub fn new(media_type: impl Into<String>, data: Vec<u8>) -> Self {
        use sha2::{Digest, Sha256};

        let digest = format!("sha256:{}", hex::encode(Sha256::digest(&data)));
        Self {
            media_type: media_type.into(),
            digest,
            size: data.len() as u64,
            data,
            annotations: BTreeMap::new(),
        }
    }

    /// Attaches descriptor annotations to the layer.
    pub fn with_annotations(mut self, annotations: BTreeMap<String, String>) -> Self {
        self.annotations = annotations;
        self
    }

    /// Returns the OCI layer media type.
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    /// Returns the digest of the compressed blob.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Returns the compressed blob size.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns the exact compressed blob bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Returns descriptor annotations attached to the layer.
    pub fn annotations(&self) -> &BTreeMap<String, String> {
        &self.annotations
    }
}

/// A local source copied into an image layer.
#[derive(Debug, Clone, Serialize)]
pub struct FileMapping {
    /// Local source path.
    pub source: PathBuf,
    /// Absolute destination path in the image.
    pub destination: String,
    /// Include globs evaluated relative to a directory source.
    pub include: Vec<String>,
    /// Exclude globs evaluated after inclusion.
    pub exclude: Vec<String>,
    /// Optional mode applied to every copied entry.
    pub mode: Option<u32>,
    /// Numeric user ID applied to every copied entry.
    pub uid: u64,
    /// Numeric group ID applied to every copied entry.
    pub gid: u64,
    /// Whether symlink targets should be copied instead of symlinks.
    pub follow_symlinks: bool,
    /// Whether the source basename is retained below the destination.
    pub preserve_paths: bool,
}

impl FileMapping {
    /// Creates a root-owned mapping with no filters or metadata overrides.
    pub fn new(source: impl Into<PathBuf>, destination: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            include: Vec::new(),
            exclude: Vec::new(),
            mode: None,
            uid: 0,
            gid: 0,
            follow_symlinks: false,
            preserve_paths: false,
        }
    }
}

/// Runtime overrides applied to an OCI image configuration.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeConfig {
    /// Entrypoint executable and fixed arguments.
    pub entrypoint: Option<Vec<String>>,
    /// Default command or entrypoint arguments.
    pub command: Option<Vec<String>>,
    /// Runtime user and optional group.
    pub user: Option<String>,
    /// Runtime working directory.
    pub workdir: Option<String>,
    /// Runtime stop signal.
    pub stop_signal: Option<String>,
    /// Exposed ports. `None` inherits from the base and an empty list clears them.
    pub expose: Option<Vec<String>>,
    /// Declared volumes. `None` inherits from the base and an empty list clears them.
    pub volumes: Option<Vec<String>>,
    /// Environment variables merged by name with inherited values.
    pub env: BTreeMap<String, String>,
    /// Image labels merged by name with inherited values.
    pub labels: BTreeMap<String, String>,
}

/// OCI output representation.
#[derive(Debug, Clone, Serialize)]
pub enum OciOutput {
    /// An unpacked OCI image layout.
    Layout(PathBuf),
    /// A tar archive containing an OCI image layout.
    Archive(PathBuf),
}

impl OciOutput {
    /// Returns the destination path.
    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::Layout(path) | Self::Archive(path) => path,
        }
    }

    /// Exports an existing OCI layout to this output.
    pub fn export_layout(
        &self,
        layout: &std::path::Path,
        source_date_epoch: u64,
    ) -> Result<(), BuildError> {
        crate::output::export_layout(layout, self, source_date_epoch)
    }
}

/// Digests produced by a completed build.
#[derive(Debug, Clone, Serialize)]
pub struct BuildResult {
    /// Digest of the top-level OCI index.
    pub index_digest: String,
    /// Digests of named image manifests or indexes.
    pub image_digests: BTreeMap<String, String>,
}
