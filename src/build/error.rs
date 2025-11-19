use std::path::PathBuf;
use thiserror::Error;

use crate::serialization::SerializeError;

use super::manifest::{ManifestError, ToolchainKind};

/// Errors surfaced while orchestrating firmware builds.
#[derive(Debug, Error)]
pub enum BuildError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("invalid build request: {0}")]
    InvalidRequest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("docker backend error: {0}")]
    Docker(String),
    #[error("operation `{0}` is not implemented yet")]
    Unimplemented(&'static str),
    #[error("failed to read manifest {path}: {source}")]
    ManifestRead {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("layout serialization failed: {0}")]
    LayoutSerialize(SerializeError),
    #[error("keyboard `{0}` not found in manifest")]
    UnknownKeyboard(String),
    #[error("toolchain `{0}` not found in manifest")]
    UnknownToolchain(String),
    #[error("keyboard `{keyboard}` target `{target}` not found")]
    UnknownTarget { keyboard: String, target: String },
    #[error("toolchain `{0:?}` is not supported yet")]
    UnsupportedToolchain(ToolchainKind),
    #[error("toolchain `{toolchain}` failed with exit code {code}")]
    CommandFailed { toolchain: String, code: i32 },
    #[error("missing staged layout artifact `{0}`")]
    MissingLayoutArtifact(&'static str),
}
