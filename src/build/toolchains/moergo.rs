use std::{fs, path::PathBuf, sync::Arc};

use crate::build::{
    docker::{DockerBackend, DockerInvocation, OutputHandler, VolumeMode, VolumeMount},
    error::BuildError,
    layout::KeymapArtifacts,
    manifest::{BuildTarget, ToolchainKind},
    progress::{LogLevel, ProgressReporter},
    request::BuildRequest,
    toolchain::{BuildContext, Toolchain, ToolchainRunResult, resolve_toolchain_config},
    workspace::WorkspaceHandle,
};

pub struct MoergoToolchain;

impl MoergoToolchain {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MoergoToolchain {
    fn default() -> Self {
        Self::new()
    }
}

impl Toolchain for MoergoToolchain {
    fn kind(&self) -> ToolchainKind {
        ToolchainKind::Moergo
    }

    fn build_target(
        &self,
        ctx: &BuildContext<'_>,
        target: &BuildTarget,
        docker: &dyn DockerBackend,
    ) -> Result<ToolchainRunResult, BuildError> {
        let config = resolve_toolchain_config(ctx.profile, target, &ctx.profile.id);
        let layout_path = select_layout_json(ctx.layout)?;
        let container_layout = ctx
            .workspace
            .container_path(&layout_path)
            .ok_or(BuildError::MissingLayoutArtifact("layout.json"))?;

        let mut env = config.env.clone();
        env.insert("MOERGO_BOARD".into(), target.board.clone());
        if let Some(shield) = &target.shield {
            env.insert("MOERGO_SHIELD".into(), shield.clone());
        }
        if let Some(variant) = &target.variant {
            env.insert("MOERGO_VARIANT".into(), variant.clone());
        }
        env.insert(
            "MOERGO_LAYOUT_JSON".into(),
            container_layout.display().to_string(),
        );
        for (key, value) in &ctx.request.extra_env {
            env.insert(key.clone(), value.clone());
        }

        let mut invocation = DockerInvocation::new(config.image);
        invocation.command = vec!["/bin/sh".into(), "-c".into(), "./build.sh".into()];
        invocation.workdir = Some(ctx.workspace.container_root().to_path_buf());
        invocation.env = env;
        invocation.volumes.push(VolumeMount {
            host_path: ctx.workspace.root().to_path_buf(),
            container_path: ctx.workspace.container_root().to_path_buf(),
            mode: VolumeMode::ReadWrite,
        });
        invocation.log_handler = Arc::new(ProgressOutputHandler::new(ctx.progress.clone()));

        let checkpoint = format!("moergo-{}", target.id);
        ctx.progress
            .start_checkpoint(&checkpoint, &format!("MoErgo build for {}", target.id));
        let status = docker.run(invocation)?;
        if !status.success() {
            ctx.progress.fail_checkpoint(&checkpoint);
            return Err(BuildError::CommandFailed {
                toolchain: ctx.profile.id.clone(),
                code: status.code,
            });
        }
        ctx.progress.complete_checkpoint(&checkpoint);

        let artifacts = collect_artifacts(ctx.workspace, ctx.request)?;
        Ok(ToolchainRunResult { artifacts })
    }
}

fn select_layout_json(artifacts: &KeymapArtifacts) -> Result<PathBuf, BuildError> {
    artifacts
        .json
        .clone()
        .ok_or(BuildError::MissingLayoutArtifact("layout.json"))
}

fn collect_artifacts(
    workspace: &WorkspaceHandle,
    request: &BuildRequest,
) -> Result<Vec<PathBuf>, BuildError> {
    fs::create_dir_all(&request.output_dir).map_err(BuildError::Io)?;
    let mut collected = Vec::new();
    if !workspace.artifacts_dir().exists() {
        return Ok(collected);
    }
    for entry in fs::read_dir(workspace.artifacts_dir()).map_err(BuildError::Io)? {
        let entry = entry.map_err(BuildError::Io)?;
        let file_type = entry.file_type().map_err(BuildError::Io)?;
        if !file_type.is_file() {
            continue;
        }
        let dest = request.output_dir.join(entry.file_name());
        fs::copy(entry.path(), &dest).map_err(BuildError::Io)?;
        collected.push(dest);
    }
    Ok(collected)
}

struct ProgressOutputHandler {
    progress: Arc<dyn ProgressReporter>,
}

impl ProgressOutputHandler {
    fn new(progress: Arc<dyn ProgressReporter>) -> Self {
        Self { progress }
    }
}

impl OutputHandler for ProgressOutputHandler {
    fn handle_stdout(&self, line: &str) {
        self.progress.log(LogLevel::Info, line);
    }

    fn handle_stderr(&self, line: &str) {
        self.progress.log(LogLevel::Warn, line);
    }
}
