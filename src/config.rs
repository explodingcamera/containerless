use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::cli::BuildPreset;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub image: ImageConfig,

    #[serde(default, rename = "artifact")]
    pub artifacts: Vec<ArtifactConfig>,

    #[serde(default)]
    pub build: Option<BuildConfig>,

    #[serde(default, rename = "platform")]
    pub platforms: Vec<PlatformConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ImageConfig {
    #[serde(default)]
    pub name: Option<String>,

    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(default)]
    pub entrypoint: Vec<String>,

    #[serde(default = "default_user")]
    pub user: String,

    #[serde(default)]
    pub env: BTreeMap<String, String>,

    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            name: None,
            tags: Vec::new(),
            entrypoint: Vec::new(),
            user: default_user(),
            env: BTreeMap::new(),
            labels: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ArtifactConfig {
    /// Source path or template, e.g. target/{{target}}/release/server.
    ///
    /// Available template variables:
    /// - {{rust_target}}: The Rust target triple, e.g. x86_64-unknown-linux-gnu
    /// - {{go_target}}: The Go target triple, e.g. linux/amd64
    /// - {{zig_target}}: The Zig target triple, e.g. x86_64-linux-gnu
    /// - {{build_system}}: The build system, e.g. cargo, go, zig
    /// - {{platform}}: The OCI platform, e.g. linux/amd64
    pub path: PathBuf,

    /// Destination path inside the image.
    pub dest: String,

    /// Optional OCI platform filter.
    ///
    /// Unset means included in every platform image. Uses OCI platform strings, e.g. linux/amd64.
    #[serde(default, deserialize_with = "deserialize_optional_string_or_vec")]
    pub platform: Option<Vec<String>>,

    #[serde(default = "default_true")]
    pub executable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct BuildConfig {
    #[serde(default)]
    pub preset: BuildPreset,

    #[serde(default)]
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PlatformConfig {
    /// OCI platform, e.g. linux/amd64.
    pub name: String,

    /// Toolchain target, e.g. x86_64-unknown-linux-gnu.
    #[serde(default)]
    pub target: Option<String>,
}

fn default_binary_dest() -> String {
    "/app/app".to_owned()
}

fn default_user() -> String {
    "nonroot".to_owned()
}

fn default_base() -> String {
    "auto".to_owned()
}

fn default_true() -> bool {
    true
}

fn deserialize_optional_string_or_vec<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    let value = Option::<OneOrMany>::deserialize(deserializer)?;

    Ok(match value {
        None => None,
        Some(OneOrMany::One(value)) => Some(vec![value]),
        Some(OneOrMany::Many(values)) => Some(values),
    })
}
