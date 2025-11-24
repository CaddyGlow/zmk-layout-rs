use std::{fs, path::PathBuf, sync::Arc};

use crate::{
    adapters::standard::AdapterLayout,
    build::{
        docker::{DockerBackend, DockerInvocation, OutputHandler, VolumeMode, VolumeMount},
        error::BuildError,
        layout::KeymapArtifacts,
        logs::LogFile,
        manifest::{BuildTarget, ToolchainKind},
        progress::{LogLevel, ProgressReporter},
        request::BuildRequest,
        toolchain::{BuildContext, Toolchain, ToolchainRunResult, resolve_toolchain_config},
        workspace::WorkspaceHandle,
    },
    dts::DtsDocument,
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
        let layout_path = ensure_layout_json(ctx.workspace, ctx.layout)?;
        let keymap_inputs = stage_moergo_inputs(ctx.workspace, ctx.layout, &target.board)?;
        let container_layout = ctx
            .workspace
            .container_path(&layout_path)
            .ok_or(BuildError::MissingLayoutArtifact("layout.json"))?;

        let mut env = config.env.clone();
        env.insert("MOERGO_BOARD".into(), target.board.clone());
        env.insert("BOARD_NAME".into(), target.board.clone());
        if let Some(shield) = &target.shield {
            env.insert("MOERGO_SHIELD".into(), shield.clone());
        }
        if let Some(variant) = &target.variant {
            env.insert("MOERGO_VARIANT".into(), variant.clone());
        }
        env.insert(
            "KEYMAP".into(),
            ctx.workspace
                .container_path(&keymap_inputs.keymap)
                .ok_or(BuildError::MissingLayoutArtifact("keymap"))?
                .display()
                .to_string(),
        );
        env.insert(
            "KCONFIG".into(),
            ctx.workspace
                .container_path(&keymap_inputs.kconfig)
                .ok_or(BuildError::MissingLayoutArtifact("kconfig"))?
                .display()
                .to_string(),
        );
        env.insert(
            "MOERGO_LAYOUT_JSON".into(),
            container_layout.display().to_string(),
        );

        // Set PUID/PGID for entrypoint.sh to handle user mapping
        #[cfg(unix)]
        {
            use nix::unistd::{Gid, Uid};
            env.insert("PUID".into(), Uid::current().as_raw().to_string());
            env.insert("PGID".into(), Gid::current().as_raw().to_string());
        }

        for (key, value) in &ctx.request.extra_env {
            env.insert(key.clone(), value.clone());
        }

        let mut invocation = DockerInvocation::new(config.image);
        invocation.command = vec!["build.sh".into()];
        invocation.env = env;
        invocation.volumes.push(VolumeMount {
            host_path: ctx.workspace.root().to_path_buf(),
            container_path: ctx.workspace.container_root().to_path_buf(),
            mode: VolumeMode::ReadWrite,
        });

        let log_label = format!("{}::{}", ctx.profile.id, target.id);
        invocation.log_handler = Arc::new(ProgressOutputHandler::new(
            ctx.progress.clone(),
            ctx.log_file.clone(),
            log_label,
        ));

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

fn ensure_layout_json(
    workspace: &WorkspaceHandle,
    artifacts: &KeymapArtifacts,
) -> Result<PathBuf, BuildError> {
    if let Some(path) = &artifacts.json {
        return Ok(path.clone());
    }
    if let Some(keymap) = &artifacts.keymap {
        let document = DtsDocument::parse_file(keymap)?;
        let layout = AdapterLayout::from_document(&document);
        let json_text = layout
            .to_standard_json()
            .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
        let dest = workspace.layout_dir().join("layout.json");
        fs::write(&dest, json_text).map_err(BuildError::Io)?;
        return Ok(dest);
    }
    Err(BuildError::MissingLayoutArtifact("layout.json"))
}

struct MoergoInputPaths {
    keymap: PathBuf,
    kconfig: PathBuf,
}

fn stage_moergo_inputs(
    workspace: &WorkspaceHandle,
    artifacts: &KeymapArtifacts,
    board: &str,
) -> Result<MoergoInputPaths, BuildError> {
    let keymap_src = artifacts
        .keymap
        .as_ref()
        .ok_or(BuildError::MissingLayoutArtifact("keymap"))?;

    let config_root = workspace.config_dir();
    fs::create_dir_all(config_root).map_err(BuildError::Io)?;

    let keymap_dest = config_root.join(format!("{board}.keymap"));
    fs::copy(keymap_src, &keymap_dest).map_err(BuildError::Io)?;
    let kconfig_dest = config_root.join(format!("{board}.conf"));
    if let Some(config_src) = artifacts.config.as_ref() {
        fs::copy(config_src, &kconfig_dest).map_err(BuildError::Io)?;
    } else {
        // Minimal kconfig for MoErgo builds can be empty; create a stub if missing.
        fs::write(&kconfig_dest, b"").map_err(BuildError::Io)?;
    }

    // Copy default.nix template to config directory
    let default_nix = include_str!("../../../toolchains/moergo/default.nix");
    let default_nix_dest = config_root.join("default.nix");
    fs::write(&default_nix_dest, default_nix).map_err(BuildError::Io)?;

    Ok(MoergoInputPaths {
        keymap: keymap_dest,
        kconfig: kconfig_dest,
    })
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
        fs::remove_file(entry.path()).map_err(BuildError::Io)?;
        collected.push(dest);
    }
    Ok(collected)
}

struct ProgressOutputHandler {
    progress: Arc<dyn ProgressReporter>,
    log: Option<LogFile>,
    stdout_tag: String,
    stderr_tag: String,
}

impl ProgressOutputHandler {
    fn new(progress: Arc<dyn ProgressReporter>, log: Option<LogFile>, label: String) -> Self {
        let stdout_tag = format!("{label}:stdout");
        let stderr_tag = format!("{label}:stderr");
        Self {
            progress,
            log,
            stdout_tag,
            stderr_tag,
        }
    }

    fn append(&self, tag: &str, line: &str) {
        if let Some(logger) = &self.log {
            logger.append(tag, line);
        }
    }
}

impl OutputHandler for ProgressOutputHandler {
    fn handle_stdout(&self, line: &str) {
        self.append(&self.stdout_tag, line);
        self.progress.log(LogLevel::Info, line);
    }

    fn handle_stderr(&self, line: &str) {
        self.append(&self.stderr_tag, line);
        self.progress.log(LogLevel::Warn, line);
    }
}
