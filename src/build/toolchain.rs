use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use super::{
    docker::DockerBackend,
    error::BuildError,
    layout::KeymapArtifacts,
    logs::LogFile,
    manifest::{
        BuildTarget, FirmwareManifest, KeyboardProfile, ToolchainKind, ToolchainOverride,
        ToolchainProfile,
    },
    progress::ProgressReporter,
    request::BuildRequest,
    workspace::WorkspaceHandle,
};

use crate::build::toolchains::{moergo::MoergoToolchain, zmk_config::ZmkConfigToolchain};

/// Context shared across toolchain invocations.
pub struct BuildContext<'a> {
    pub manifest: &'a FirmwareManifest,
    pub keyboard: &'a KeyboardProfile,
    pub profile: &'a ToolchainProfile,
    pub request: &'a BuildRequest,
    pub workspace: &'a WorkspaceHandle,
    pub layout: &'a KeymapArtifacts,
    pub progress: Arc<dyn ProgressReporter>,
    pub log_file: Option<LogFile>,
}

/// Result returned by a toolchain run.
pub struct ToolchainRunResult {
    pub artifacts: Vec<PathBuf>,
}

pub trait Toolchain {
    fn kind(&self) -> ToolchainKind;
    fn build_target(
        &self,
        ctx: &BuildContext<'_>,
        target: &BuildTarget,
        docker: &dyn DockerBackend,
    ) -> Result<ToolchainRunResult, BuildError>;
}

pub fn create_toolchain(profile: &ToolchainProfile) -> Result<Box<dyn Toolchain>, BuildError> {
    match profile.kind {
        ToolchainKind::Moergo => Ok(Box::new(MoergoToolchain::new())),
        ToolchainKind::ZmkConfig => Ok(Box::new(ZmkConfigToolchain::new())),
    }
}

/// Toolchain configuration derived from manifest + target overrides.
pub struct ToolchainConfig {
    pub image: String,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub env: BTreeMap<String, String>,
}

pub fn resolve_toolchain_config(
    profile: &ToolchainProfile,
    target: &BuildTarget,
    toolchain_id: &str,
) -> ToolchainConfig {
    let mut image = profile.image.clone();
    let mut repository = profile.repository.clone();
    let mut branch = profile.branch.clone();
    let mut env = profile.env.clone();

    if let Some(repo) = &target.repo_override {
        repository = Some(repo.clone());
    }
    if let Some(value) = &target.branch_override {
        branch = Some(value.clone());
    }

    if let Some(overrides) = target.toolchain_overrides.get(toolchain_id) {
        apply_toolchain_override(
            &mut image,
            &mut repository,
            &mut branch,
            &mut env,
            overrides,
        );
    }

    ToolchainConfig {
        image,
        repository,
        branch,
        env,
    }
}

fn apply_toolchain_override(
    image: &mut String,
    repository: &mut Option<String>,
    branch: &mut Option<String>,
    env: &mut BTreeMap<String, String>,
    overrides: &ToolchainOverride,
) {
    if let Some(value) = &overrides.image {
        *image = value.clone();
    }
    if let Some(value) = &overrides.repository {
        *repository = Some(value.clone());
    }
    if let Some(value) = &overrides.branch {
        *branch = Some(value.clone());
    }
    for (key, value) in &overrides.env {
        env.insert(key.clone(), value.clone());
    }
}
