use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::{Builder as TempDirBuilder, TempDir};

use super::error::BuildError;

const CONTAINER_ROOT: &str = "/workspace";

/// Manages ephemeral workspaces for firmware builds.
#[derive(Debug, Default)]
pub struct WorkspaceManager;

impl WorkspaceManager {
    pub fn new() -> Self {
        Self
    }

    pub fn create_workspace(&self, toolchain_id: &str) -> Result<WorkspaceHandle, BuildError> {
        let dir = TempDirBuilder::new()
            .prefix(&format!("zmk-{toolchain_id}-"))
            .tempdir()
            .map_err(BuildError::Io)?;

        let layout_dir = dir.path().join("layout");
        let artifacts_dir = dir.path().join("artifacts");
        let config_dir = dir.path().join("config");
        let app_dir = dir.path().join("app");
        let build_dir = dir.path().join("build");

        fs::create_dir_all(&layout_dir).map_err(BuildError::Io)?;
        fs::create_dir_all(&artifacts_dir).map_err(BuildError::Io)?;
        fs::create_dir_all(&config_dir).map_err(BuildError::Io)?;
        fs::create_dir_all(&app_dir).map_err(BuildError::Io)?;
        fs::create_dir_all(&build_dir).map_err(BuildError::Io)?;

        Ok(WorkspaceHandle {
            root: dir,
            layout_dir,
            artifacts_dir,
            config_dir,
            app_dir,
            build_dir,
        })
    }
}

/// Handle referencing an on-disk workspace.
pub struct WorkspaceHandle {
    root: TempDir,
    layout_dir: PathBuf,
    artifacts_dir: PathBuf,
    config_dir: PathBuf,
    app_dir: PathBuf,
    build_dir: PathBuf,
}

impl WorkspaceHandle {
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn layout_dir(&self) -> &Path {
        &self.layout_dir
    }

    pub fn artifacts_dir(&self) -> &Path {
        &self.artifacts_dir
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn app_dir(&self) -> &Path {
        &self.app_dir
    }

    pub fn build_dir(&self) -> &Path {
        &self.build_dir
    }

    pub fn build_dir_for(&self, target_id: &str) -> PathBuf {
        self.build_dir.join(target_id)
    }

    pub fn container_root(&self) -> &Path {
        Path::new(CONTAINER_ROOT)
    }

    pub fn container_path(&self, host_path: &Path) -> Option<PathBuf> {
        host_path
            .strip_prefix(self.root())
            .ok()
            .map(|rel| self.container_root().join(rel))
    }
}
