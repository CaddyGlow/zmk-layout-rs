//! High-level firmware builder facade used by the CLI/library.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use super::{
    docker::DockerBackend,
    error::BuildError,
    layout::LayoutStager,
    manifest::{BuildTarget, FirmwareManifest, KeyboardProfile},
    progress::ProgressReporter,
    request::{BuildRequest, BuildRequestBuilder},
    toolchain::{BuildContext, create_toolchain},
    workspace::WorkspaceManager,
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

    pub fn build(&self, request: BuildRequest) -> Result<BuildReport, BuildError> {
        let keyboard = self
            .manifest
            .keyboards
            .get(&request.keyboard_id)
            .ok_or_else(|| BuildError::UnknownKeyboard(request.keyboard_id.clone()))?;
        let toolchain_id = request
            .toolchain_id
            .clone()
            .unwrap_or_else(|| keyboard.default_toolchain.clone());
        let profile = self
            .manifest
            .toolchains
            .get(&toolchain_id)
            .ok_or_else(|| BuildError::UnknownToolchain(toolchain_id.clone()))?;
        let toolchain = create_toolchain(profile)?;

        let workspace_manager = WorkspaceManager::new();
        let workspace = workspace_manager.create_workspace(&profile.id)?;
        let stager = LayoutStager::new();
        let layout = stager.stage(&request.layout, &workspace)?;

        let ctx = BuildContext {
            manifest: &self.manifest,
            keyboard,
            profile,
            request: &request,
            workspace: &workspace,
            layout: &layout,
            progress: request.progress.clone(),
        };

        let mut report = BuildReport::default();
        report.success = true;
        for target_ref in &request.targets {
            let target = resolve_target(keyboard, &target_ref.id)?;
            let result = toolchain.build_target(&ctx, target, self.docker())?;
            report.artifacts.files.extend(result.artifacts);
        }

        Ok(report)
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

impl ProgressReporter for CliProgressReporter {
    fn log(&self, level: super::LogLevel, message: &str) {
        eprintln!("[{level:?}] {message}");
    }

    fn start_checkpoint(&self, id: &str, message: &str) {
        eprintln!("[START] {id}: {message}");
    }

    fn complete_checkpoint(&self, id: &str) {
        eprintln!("[DONE ] {id}");
    }

    fn fail_checkpoint(&self, id: &str) {
        eprintln!("[FAIL ] {id}");
    }
}

fn resolve_target<'a>(
    keyboard: &'a KeyboardProfile,
    id: &str,
) -> Result<&'a BuildTarget, BuildError> {
    keyboard
        .targets
        .iter()
        .find(|target| target.id == id)
        .ok_or_else(|| BuildError::UnknownTarget {
            keyboard: keyboard.id.clone(),
            target: id.to_string(),
        })
}
