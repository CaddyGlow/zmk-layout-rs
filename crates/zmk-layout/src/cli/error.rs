use std::path::PathBuf;

use thiserror::Error;

#[cfg(feature = "ancpp-preprocessor")]
use zmk_layout_core::preprocessor::AncppError;
use zmk_layout_core::{
    adapters::AdapterError,
    build::{BuildError, BuildRequestError, ManifestError},
    flash::FlashError,
    io::IoError,
    serialization::SerializeError,
    tasks::TaskConfigError,
};
use zmk_layout_lua::ScriptExecutionError;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse task file: {0}")]
    TaskConfig(#[from] TaskConfigError),
    #[error("failed to parse layout {path}: {source}")]
    ParseLayout {
        path: PathBuf,
        source: zmk_layout_core::tokenizer::LayoutError,
    },
    #[error("failed to serialize layout: {0}")]
    Serialize(SerializeError),
    #[error("failed to parse firmware manifest: {0}")]
    Manifest(#[from] ManifestError),
    #[error("invalid firmware build request: {0}")]
    FirmwareRequest(#[from] BuildRequestError),
    #[error("firmware build failed: {0}")]
    FirmwareBuild(#[from] BuildError),
    #[error("invalid firmware layout arguments: {0}")]
    FirmwareLayout(String),
    #[error("invalid env specification `{0}`, expected KEY=VALUE")]
    InvalidEnv(String),
    #[error("invalid kconfig definition `{0}`, expected NAME=VALUE")]
    InvalidKconfigDef(String),
    #[error("script execution error: {0}")]
    ScriptExecution(#[from] ScriptExecutionError),
    #[error("profile check error: {0}")]
    ProfileCheck(String),
    #[error("keyboard `{0}` not found in manifest")]
    UnknownKeyboard(String),
    #[error("keyboard `{0}` has no metadata.profile reference in the manifest")]
    MissingKeyboardProfile(String),
    #[error("flashing failed: {0}")]
    Flash(#[from] FlashError),
    #[error("adapter error: {0}")]
    Adapter(#[from] AdapterError),
    #[error("invalid arguments: {0}")]
    InvalidArgument(String),
    #[cfg(feature = "ancpp-preprocessor")]
    #[error("failed to preprocess layout {path}: {source}")]
    PreprocessLayout { path: PathBuf, source: AncppError },
}

impl From<IoError> for CliError {
    fn from(err: IoError) -> Self {
        match err {
            IoError::ReadFile { path, source } => CliError::ReadFile { path, source },
            IoError::WriteFile { path, source } => CliError::WriteFile { path, source },
            IoError::ParseLayout { path, source } => CliError::ParseLayout { path, source },
            IoError::ParseTaskFile { source, .. } => CliError::TaskConfig(source),
            IoError::SerializeLayout(err) => CliError::Serialize(err),
            IoError::Adapter(err) => CliError::Adapter(err),
            #[cfg(feature = "ancpp-preprocessor")]
            IoError::PreprocessLayout { path, source } => {
                CliError::PreprocessLayout { path, source }
            }
        }
    }
}
