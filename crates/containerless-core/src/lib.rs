//! Build OCI images from files.

mod error;
mod flatten;
mod image;
mod layer;
mod model;
mod output;

pub use error::{BuildError, PlatformParseError};
pub use model::*;
