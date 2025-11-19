//! High-level firmware builder facade used by the CLI/library.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use super::{
    docker::DockerBackend,
    error::BuildError,
    manifest::FirmwareManifest,
    progress::ProgressReporter,
    request::{BuildRequest, BuildRequestBuilder},
};

/// Firmware build coordinator – later phases will wire toolchains here.
pub struct FirmwareBuilder {
    manifest: Arc<FirmwareManifest>,
    docker: Box<dyn DockerBackend>,
}

impl FirmwareBuilder {
    pub fn new(manifest: FirmwareManifest, docker: Box<dyn DockerBackend>) -> Self {
        Self {
            manifest: Arc::new(manifest),
            docker,
        }
    }

    pub fn builder(&self) -> BuildRequestBuilder {
        BuildRequestBuilder::new(self.manifest.clone())
    }

    pub fn docker(&self) -> &dyn DockerBackend {
        self.docker.as_ref()
    }

    pub fn manifest(&self) -> &FirmwareManifest {
        &self.manifest
    }

    pub fn build(&self, _request: BuildRequest) -> Result<BuildReport, BuildError> {
        Err(BuildError::Unimplemented("firmware build pipeline"))
    }
}

/// Build report returned by [`FirmwareBuilder::build`].
#[derive(Debug, Clone)]
pub struct BuildReport {
    pub success: bool,
    pub artifacts: ArtifactReport,
    pub logs_path: Option<PathBuf>,
    pub metadata: BuildMetadata,
}

impl Default for BuildReport {
    fn default() -> Self {
        Self {
            success: false,
            artifacts: ArtifactReport::default(),
            logs_path: None,
            metadata: BuildMetadata::default(),
        }
    }
}

/// Collection of build artifacts (.uf2, .bin, logs, etc.).
#[derive(Debug, Clone, Default)]
pub struct ArtifactReport {
    pub files: Vec<PathBuf>,
}

/// Extra metadata captured during a build (toolchain info, timings, etc.).
#[derive(Debug, Clone, Default)]
pub struct BuildMetadata {
    pub entries: BTreeMap<String, String>,
}

/// Placeholder progress reporter used by the CLI.
pub struct CliProgressReporter;

impl ProgressReporter for CliProgressReporter {}
