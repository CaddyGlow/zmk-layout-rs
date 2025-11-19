use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::build::{
    docker::{DockerBackend, DockerInvocation, OutputHandler, VolumeMode, VolumeMount},
    error::BuildError,
    layout::KeymapArtifacts,
    logs::LogFile,
    manifest::{BuildTarget, ToolchainKind},
    progress::{LogLevel, ProgressReporter},
    request::BuildRequest,
    toolchain::{
        BuildContext, Toolchain, ToolchainConfig, ToolchainRunResult, resolve_toolchain_config,
    },
    workspace::WorkspaceHandle,
};

const DEFAULT_REPOSITORY: &str = "https://github.com/zmkfirmware/zmk.git";
const DEFAULT_REPOSITORY_ID: &str = "zmkfirmware/zmk";
const DEFAULT_BRANCH: &str = "main";
const WORKSPACE_STATE_FILE: &str = ".zmk-workspace.json";

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

        let mut env = assemble_env(&config.env, &ctx.request.extra_env);
        let zmk_config = ctx
            .workspace
            .container_path(ctx.workspace.config_dir())
            .unwrap_or_else(|| PathBuf::from("/workspace/config"));
        env.insert("ZMK_CONFIG".into(), zmk_config.display().to_string());

        let checkpoint = format!("zmk-{}", target.id);
        ctx.progress
            .start_checkpoint(&checkpoint, &format!("ZMK build for {}", target.id));
        let build_result = (|| {
            self.ensure_workspace(ctx, &config, docker)?;
            self.run_west_build(ctx, target, docker, &config, env, &build_dir)
        })();
        if let Err(err) = build_result {
            ctx.progress.fail_checkpoint(&checkpoint);
            return Err(err);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceState {
    repository: String,
    branch: String,
}

#[derive(Debug, Clone)]
struct RepoSpec {
    url: String,
    display: String,
    branch: String,
}

impl RepoSpec {
    fn new(repo: &str, branch: &str) -> Self {
        let (url, display) = normalize_repository(repo);
        let branch = if branch.trim().is_empty() {
            DEFAULT_BRANCH.to_string()
        } else {
            branch.to_string()
        };
        Self {
            url,
            display,
            branch,
        }
    }
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

impl ZmkConfigToolchain {
    fn ensure_workspace(
        &self,
        ctx: &BuildContext<'_>,
        config: &ToolchainConfig,
        docker: &dyn DockerBackend,
    ) -> Result<(), BuildError> {
        let repo_value = config.repository.as_deref().unwrap_or(DEFAULT_REPOSITORY);
        let branch_value = config.branch.as_deref().unwrap_or(DEFAULT_BRANCH);
        let spec = RepoSpec::new(repo_value, branch_value);

        let app_dir = ctx.workspace.app_dir();
        let container_app_dir = ctx
            .workspace
            .container_path(app_dir)
            .unwrap_or_else(|| PathBuf::from("/workspace/app"));
        let env = assemble_env(&config.env, &ctx.request.extra_env);

        if workspace_requires_init(app_dir, &spec) {
            reset_directory(app_dir)?;
            self.run_west_init(ctx, docker, &config.image, &env, &spec, &container_app_dir)?;
            write_workspace_state(app_dir, &spec)?;
        }

        self.run_west_command(
            ctx,
            docker,
            &config.image,
            &env,
            vec!["west".into(), "update".into()],
            Some(container_app_dir.clone()),
            format!("{}::update", ctx.profile.id),
        )?;
        self.run_west_command(
            ctx,
            docker,
            &config.image,
            &env,
            vec!["west".into(), "zephyr-export".into()],
            Some(container_app_dir),
            format!("{}::zephyr-export", ctx.profile.id),
        )?;
        Ok(())
    }

    fn run_west_build(
        &self,
        ctx: &BuildContext<'_>,
        target: &BuildTarget,
        docker: &dyn DockerBackend,
        config: &ToolchainConfig,
        env: BTreeMap<String, String>,
        build_dir: &Path,
    ) -> Result<(), BuildError> {
        let container_app_dir = ctx
            .workspace
            .container_path(ctx.workspace.app_dir())
            .unwrap_or_else(|| PathBuf::from("/workspace/app"));
        let container_build_dir = ctx
            .workspace
            .container_path(build_dir)
            .unwrap_or_else(|| PathBuf::from("/workspace/build"));
        let config_path = ctx
            .workspace
            .container_path(ctx.workspace.config_dir())
            .unwrap_or_else(|| PathBuf::from("/workspace/config"));

        let mut command = vec![
            "west".into(),
            "build".into(),
            "-s".into(),
            container_app_dir.join("zmk/app").display().to_string(),
            "-d".into(),
            container_build_dir.display().to_string(),
            "-b".into(),
            target.board.clone(),
        ];

        let mut cmake_args = vec![format!("-DZMK_CONFIG={}", config_path.display())];
        if let Some(shield) = &target.shield {
            cmake_args.push(format!("-DSHIELD={shield}"));
        }
        for (key, value) in &target.cmake_defs {
            cmake_args.push(format!("-D{key}={value}"));
        }

        if !cmake_args.is_empty() {
            command.push("--".into());
            command.extend(cmake_args);
        }

        self.run_west_command(
            ctx,
            docker,
            &config.image,
            &env,
            command,
            ctx.workspace.container_path(ctx.workspace.app_dir()),
            format!("{}::{}", ctx.profile.id, target.id),
        )?;

        Ok(())
    }

    fn run_west_init(
        &self,
        ctx: &BuildContext<'_>,
        docker: &dyn DockerBackend,
        image: &str,
        env: &BTreeMap<String, String>,
        spec: &RepoSpec,
        container_app_dir: &Path,
    ) -> Result<(), BuildError> {
        let command = vec![
            "west".into(),
            "init".into(),
            "-m".into(),
            spec.url.clone(),
            "--mr".into(),
            spec.branch.clone(),
            container_app_dir.display().to_string(),
        ];
        self.run_west_command(
            ctx,
            docker,
            image,
            env,
            command,
            Some(ctx.workspace.container_root().to_path_buf()),
            format!("{}::init", ctx.profile.id),
        )
    }

    fn run_west_command(
        &self,
        ctx: &BuildContext<'_>,
        docker: &dyn DockerBackend,
        image: &str,
        env: &BTreeMap<String, String>,
        command: Vec<String>,
        workdir: Option<PathBuf>,
        label: String,
    ) -> Result<(), BuildError> {
        let mut invocation = DockerInvocation::new(image.to_string());
        invocation.command = command;
        invocation.env = env.clone();
        invocation.workdir = workdir;
        invocation.volumes.push(VolumeMount {
            host_path: ctx.workspace.root().to_path_buf(),
            container_path: ctx.workspace.container_root().to_path_buf(),
            mode: VolumeMode::ReadWrite,
        });
        invocation.log_handler = Arc::new(ProgressOutputHandler::new(
            ctx.progress.clone(),
            ctx.log_file.clone(),
            label,
        ));
        let status = docker.run(invocation)?;
        if !status.success() {
            return Err(BuildError::CommandFailed {
                toolchain: ctx.profile.id.clone(),
                code: status.code,
            });
        }
        Ok(())
    }
}

fn workspace_requires_init(app_dir: &Path, spec: &RepoSpec) -> bool {
    let state_path = app_dir.join(WORKSPACE_STATE_FILE);
    let west_config = app_dir.join(".west/config");
    if !west_config.exists() || !state_path.exists() {
        return true;
    }
    match fs::read_to_string(state_path) {
        Ok(text) => match serde_json::from_str::<WorkspaceState>(&text) {
            Ok(state) => state.repository != spec.display || state.branch != spec.branch,
            Err(_) => true,
        },
        Err(_) => true,
    }
}

fn write_workspace_state(app_dir: &Path, spec: &RepoSpec) -> Result<(), BuildError> {
    let state = WorkspaceState {
        repository: spec.display.clone(),
        branch: spec.branch.clone(),
    };
    let text =
        serde_json::to_string(&state).map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
    fs::write(app_dir.join(WORKSPACE_STATE_FILE), text).map_err(BuildError::Io)
}

fn reset_directory(path: &Path) -> Result<(), BuildError> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(BuildError::Io)?;
    }
    fs::create_dir_all(path).map_err(BuildError::Io)
}

fn normalize_repository(input: &str) -> (String, String) {
    let trimmed = input.trim().trim_end_matches('/');
    let url = if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("git@")
        || trimmed.starts_with("git://")
    {
        trimmed.to_string()
    } else if trimmed.contains('/') {
        format!("https://github.com/{trimmed}")
    } else {
        format!("https://github.com/{DEFAULT_REPOSITORY_ID}")
    };

    let mut display = url.clone();
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "git@github.com:",
        "git://github.com/",
    ] {
        if display.starts_with(prefix) {
            display = display[prefix.len()..].to_string();
            break;
        }
    }
    if display.ends_with(".git") {
        display.truncate(display.len() - 4);
    }
    if display.is_empty() {
        display = DEFAULT_REPOSITORY_ID.to_string();
    }
    (url, display)
}
