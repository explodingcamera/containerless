use std::io;

use thiserror::Error;

/// An invalid OCI platform string.
#[derive(Debug, Error)]
#[error("invalid OCI platform {value:?}; expected OS/ARCH[/VARIANT]")]
pub struct PlatformParseError {
    pub(crate) value: String,
}

/// An error produced while resolving files or constructing an OCI image.
#[derive(Debug, Error)]
pub enum BuildError {
    /// A filesystem or archive operation failed.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// OCI metadata could not be serialized.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// An image definition is invalid.
    #[error("{0}")]
    Invalid(String),
    /// An image uses an unsupported feature.
    #[error("{0}")]
    Unsupported(String),
}
