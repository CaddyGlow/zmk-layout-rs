//! Docker backend abstractions used by firmware toolchains.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use super::{
    error::BuildError,
    progress::{NoopProgressReporter, ProgressReporter},
};

/// Interface for invoking Docker commands.
pub trait DockerBackend: Send + Sync {
    fn ensure_available(&self) -> Result<(), BuildError>;
    fn run(&self, invocation: DockerInvocation) -> Result<ProcessStatus, BuildError>;
    fn build(&self, opts: DockerBuildOptions) -> Result<(), BuildError>;
}

/// Minimal CLI-based Docker backend placeholder.
pub struct CliDockerBackend {
    binary: PathBuf,
    checked: AtomicBool,
}

impl CliDockerBackend {
    pub fn new() -> Self {
        Self {
            binary: PathBuf::from("docker"),
            checked: AtomicBool::new(false),
        }
    }

    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        Self {
            binary: path.into(),
            checked: AtomicBool::new(false),
        }
    }

    fn binary(&self) -> &Path {
        &self.binary
    }
}

impl DockerBackend for CliDockerBackend {
    fn ensure_available(&self) -> Result<(), BuildError> {
        if self.checked.load(Ordering::SeqCst) {
            return Ok(());
        }
        let status = Command::new(self.binary())
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|err| BuildError::Docker(err.to_string()))?;
        if !status.success() {
            return Err(BuildError::Docker(
                "docker binary is not available or failed to run".into(),
            ));
        }
        self.checked.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn run(&self, invocation: DockerInvocation) -> Result<ProcessStatus, BuildError> {
        self.ensure_available()?;
        let mut cmd = Command::new(self.binary());
        cmd.arg("run").arg("--rm");
        if let Some(entrypoint) = &invocation.entrypoint {
            cmd.arg("--entrypoint").arg(entrypoint);
        }
        if let Some(workdir) = invocation.workdir.as_ref() {
            cmd.arg("-w").arg(workdir);
        }
        if let Some(user) = invocation.user {
            cmd.arg("-u").arg(format!("{}:{}", user.uid, user.gid));
        }
        for (key, value) in &invocation.env {
            cmd.arg("-e").arg(format!("{key}={value}"));
        }
        for volume in &invocation.volumes {
            cmd.arg("-v").arg(format_volume(volume)?);
        }
        cmd.arg(&invocation.image);
        cmd.args(&invocation.command);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|err| BuildError::Docker(err.to_string()))?;
        let handler = invocation.log_handler.clone();
        if let Some(stdout) = child.stdout.take() {
            let handler = handler.clone();
            thread::spawn(move || pump_output(stdout, handler, true));
        }
        if let Some(stderr) = child.stderr.take() {
            let handler = handler.clone();
            thread::spawn(move || pump_output(stderr, handler, false));
        }
        let status = child
            .wait()
            .map_err(|err| BuildError::Docker(err.to_string()))?;
        Ok(ProcessStatus {
            code: status.code().unwrap_or(-1),
        })
    }

    fn build(&self, opts: DockerBuildOptions) -> Result<(), BuildError> {
        self.ensure_available()?;
        let mut cmd = Command::new(self.binary());
        cmd.arg("build");
        if let Some(dockerfile) = &opts.dockerfile {
            cmd.arg("-f").arg(dockerfile);
        }
        if let Some(tag) = &opts.tag {
            cmd.arg("-t").arg(tag);
        }
        for (key, value) in &opts.build_args {
            cmd.arg("--build-arg").arg(format!("{key}={value}"));
        }
        cmd.arg(&opts.context);
        let status = cmd
            .status()
            .map_err(|err| BuildError::Docker(err.to_string()))?;
        if !status.success() {
            return Err(BuildError::Docker("docker build failed".into()));
        }
        Ok(())
    }
}

/// Process completion metadata returned by Docker invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessStatus {
    pub code: i32,
}

impl ProcessStatus {
    pub fn success(&self) -> bool {
        self.code == 0
    }
}

/// `docker run` invocation description.
pub struct DockerInvocation {
    pub image: String,
    pub command: Vec<String>,
    pub entrypoint: Option<String>,
    pub env: BTreeMap<String, String>,
    pub volumes: Vec<VolumeMount>,
    pub workdir: Option<PathBuf>,
    pub user: Option<DockerUser>,
    pub log_handler: Arc<dyn OutputHandler>,
}

impl DockerInvocation {
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            command: Vec::new(),
            entrypoint: None,
            env: BTreeMap::new(),
            volumes: Vec::new(),
            workdir: None,
            user: None,
            log_handler: Arc::new(NullOutputHandler),
        }
    }
}

/// Docker build options for the CLI backend.
pub struct DockerBuildOptions {
    pub context: PathBuf,
    pub dockerfile: Option<PathBuf>,
    pub tag: Option<String>,
    pub build_args: BTreeMap<String, String>,
    pub progress: Arc<dyn ProgressReporter>,
}

impl DockerBuildOptions {
    pub fn new(context: impl Into<PathBuf>) -> Self {
        Self {
            context: context.into(),
            dockerfile: None,
            tag: None,
            build_args: BTreeMap::new(),
            progress: Arc::new(NoopProgressReporter::new()),
        }
    }
}

/// Volume mapping definition.
#[derive(Debug, Clone)]
pub struct VolumeMount {
    pub host_path: PathBuf,
    pub container_path: PathBuf,
    pub mode: VolumeMode,
}

/// Volume mount permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeMode {
    ReadOnly,
    ReadWrite,
}

/// User override for Docker invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockerUser {
    pub uid: u32,
    pub gid: u32,
}

/// Log handler receiving stdout/stderr lines from Docker.
pub trait OutputHandler: Send + Sync {
    fn handle_stdout(&self, _line: &str) {}
    fn handle_stderr(&self, _line: &str) {}
}

/// Default handler that discards output.
#[derive(Debug, Default)]
pub struct NullOutputHandler;

impl OutputHandler for NullOutputHandler {}

fn format_volume(mount: &VolumeMount) -> Result<String, BuildError> {
    let host = mount
        .host_path
        .canonicalize()
        .unwrap_or_else(|_| mount.host_path.clone());
    let mut spec = format!("{}:{}", host.display(), mount.container_path.display());
    if mount.mode == VolumeMode::ReadOnly {
        spec.push_str(":ro");
    }
    Ok(spec)
}

fn pump_output<R: std::io::Read + Send + 'static>(
    reader: R,
    handler: Arc<dyn OutputHandler>,
    stdout: bool,
) {
    let buf = BufReader::new(reader);
    for line in buf.lines() {
        match line {
            Ok(text) => {
                if stdout {
                    handler.handle_stdout(&text);
                } else {
                    handler.handle_stderr(&text);
                }
            }
            Err(_) => break,
        }
    }
}
