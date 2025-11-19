use std::path::PathBuf;
use thiserror::Error;

use super::manifest::ManifestError;

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
}
