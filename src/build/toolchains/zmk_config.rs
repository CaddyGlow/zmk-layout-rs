use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

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

pub struct ZmkConfigToolchain;

impl ZmkConfigToolchain {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ZmkConfigToolchain {
    fn default() -> Self {
        Self::new()
    }
}

impl Toolchain for ZmkConfigToolchain {
    fn kind(&self) -> ToolchainKind {
        ToolchainKind::ZmkConfig
    }

    fn build_target(
        &self,
        ctx: &BuildContext<'_>,
        target: &BuildTarget,
        docker: &dyn DockerBackend,
    ) -> Result<ToolchainRunResult, BuildError> {
        place_layout_files(ctx.workspace, ctx.layout, target)?;
        let config = resolve_toolchain_config(ctx.profile, target, &ctx.profile.id);
        let build_dir = ctx.workspace.build_dir_for(&target.id);
        fs::create_dir_all(&build_dir).map_err(BuildError::Io)?;

        let mut command = vec![
            "west".into(),
            "build".into(),
            "-s".into(),
            ctx.workspace
                .container_path(ctx.workspace.app_dir())
                .unwrap_or_else(|| PathBuf::from("/workspace/app"))
                .display()
                .to_string(),
            "-d".into(),
            ctx.workspace
                .container_path(&build_dir)
                .unwrap_or_else(|| PathBuf::from("/workspace/build"))
                .display()
                .to_string(),
            "-b".into(),
            target.board.clone(),
        ];

        if let Some(shield) = &target.shield {
            command.push(format!("-DSHIELD={shield}"));
        }
        for (key, value) in &target.cmake_defs {
            command.push(format!("-D{key}={value}"));
        }

        let mut env = assemble_env(&config.env, &ctx.request.extra_env);
        env.insert(
            "ZMK_CONFIG".into(),
            ctx.workspace
                .container_path(ctx.workspace.config_dir())
                .unwrap_or_else(|| PathBuf::from("/workspace/config"))
                .display()
                .to_string(),
        );

        let mut invocation = DockerInvocation::new(config.image);
        invocation.command = command;
        invocation.workdir = ctx.workspace.container_path(ctx.workspace.app_dir());
        invocation.env = env;
        invocation.volumes.push(VolumeMount {
            host_path: ctx.workspace.root().to_path_buf(),
            container_path: ctx.workspace.container_root().to_path_buf(),
            mode: VolumeMode::ReadWrite,
        });
        invocation.log_handler = Arc::new(ProgressOutputHandler::new(ctx.progress.clone()));

        let checkpoint = format!("zmk-{}", target.id);
        ctx.progress
            .start_checkpoint(&checkpoint, &format!("ZMK build for {}", target.id));
        let status = docker.run(invocation)?;
        if !status.success() {
            ctx.progress.fail_checkpoint(&checkpoint);
            return Err(BuildError::CommandFailed {
                toolchain: ctx.profile.id.clone(),
                code: status.code,
            });
        }
        ctx.progress.complete_checkpoint(&checkpoint);

        let artifacts = collect_build_artifacts(ctx.workspace, target, ctx.request)?;
        Ok(ToolchainRunResult { artifacts })
    }
}

fn assemble_env(
    base: &BTreeMap<String, String>,
    overrides: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut merged = base.clone();
    for (key, value) in overrides {
        merged.insert(key.clone(), value.clone());
    }
    merged
}

fn place_layout_files(
    workspace: &WorkspaceHandle,
    layout: &KeymapArtifacts,
    target: &BuildTarget,
) -> Result<(), BuildError> {
    let keymap_src = layout
        .keymap
        .as_ref()
        .ok_or(BuildError::MissingLayoutArtifact("keymap"))?;
    let config_root = workspace.config_dir();
    if let Some(shield) = &target.shield {
        let shield_dir = config_root.join("boards/shields").join(shield);
        fs::create_dir_all(&shield_dir).map_err(BuildError::Io)?;
        let keymap_dest = shield_dir.join(format!("{shield}.keymap"));
        fs::copy(keymap_src, &keymap_dest).map_err(BuildError::Io)?;
        if let Some(config_src) = layout.config.as_ref() {
            let config_dest = shield_dir.join(format!("{shield}.conf"));
            fs::copy(config_src, &config_dest).map_err(BuildError::Io)?;
        }
    } else {
        let board_dir = config_root.join("boards").join(&target.board);
        fs::create_dir_all(&board_dir).map_err(BuildError::Io)?;
        let keymap_dest = board_dir.join(format!("{}.keymap", target.board));
        fs::copy(keymap_src, &keymap_dest).map_err(BuildError::Io)?;
        if let Some(config_src) = layout.config.as_ref() {
            let config_dest = board_dir.join(format!("{}.conf", target.board));
            fs::copy(config_src, &config_dest).map_err(BuildError::Io)?;
        }
    }
    Ok(())
}

fn collect_build_artifacts(
    workspace: &WorkspaceHandle,
    target: &BuildTarget,
    request: &BuildRequest,
) -> Result<Vec<PathBuf>, BuildError> {
    let build_dir = workspace.build_dir_for(&target.id).join("zephyr");
    if !build_dir.exists() {
        return Ok(Vec::new());
    }
    fs::create_dir_all(&request.output_dir).map_err(BuildError::Io)?;
    let mut files = Vec::new();
    for entry in fs::read_dir(&build_dir).map_err(BuildError::Io)? {
        let entry = entry.map_err(BuildError::Io)?;
        let ty = entry.file_type().map_err(BuildError::Io)?;
        if !ty.is_file() {
            continue;
        }
        let path = entry.path();
        if !matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("uf2" | "bin" | "hex")
        ) {
            continue;
        }
        let dest = request.output_dir.join(entry.file_name());
        fs::copy(&path, &dest).map_err(BuildError::Io)?;
        files.push(dest);
    }
    Ok(files)
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
