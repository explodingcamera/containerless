//! Containerless configuration and orchestration.
//!
//! This crate translates config and CLI inputs into the config-independent operations exposed by
//! [`core`]. It is also the library facade used by `cargo-containerless`.

pub mod cli;
pub mod config;

pub use containerless_core as core;
