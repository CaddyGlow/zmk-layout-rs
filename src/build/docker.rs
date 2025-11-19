//! Docker backend abstractions used by firmware toolchains.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
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
}

impl CliDockerBackend {
    pub fn new() -> Self {
        Self {
            binary: PathBuf::from("docker"),
        }
    }

    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        Self {
            binary: path.into(),
        }
    }

    fn binary(&self) -> &Path {
        &self.binary
    }
}

impl DockerBackend for CliDockerBackend {
    fn ensure_available(&self) -> Result<(), BuildError> {
        let _ = self.binary();
        Ok(())
    }

    fn run(&self, _invocation: DockerInvocation) -> Result<ProcessStatus, BuildError> {
        Err(BuildError::Unimplemented("docker run"))
    }

    fn build(&self, _opts: DockerBuildOptions) -> Result<(), BuildError> {
        Err(BuildError::Unimplemented("docker build"))
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
    pub log_handler: Box<dyn OutputHandler>,
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
            log_handler: Box::new(NullOutputHandler),
        }
    }
}

/// Docker build options for the CLI backend.
pub struct DockerBuildOptions {
    pub context: PathBuf,
    pub dockerfile: Option<PathBuf>,
    pub tag: Option<String>,
    pub build_args: BTreeMap<String, String>,
    pub progress: Box<dyn ProgressReporter>,
}

impl DockerBuildOptions {
    pub fn new(context: impl Into<PathBuf>) -> Self {
        Self {
            context: context.into(),
            dockerfile: None,
            tag: None,
            build_args: BTreeMap::new(),
            progress: Box::new(NoopProgressReporter::new()),
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
pub trait OutputHandler: Send {
    fn handle_stdout(&self, _line: &str) {}
    fn handle_stderr(&self, _line: &str) {}
}

/// Default handler that discards output.
#[derive(Debug, Default)]
pub struct NullOutputHandler;

impl OutputHandler for NullOutputHandler {}
