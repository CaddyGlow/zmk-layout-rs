use std::{
    collections::hash_map::DefaultHasher,
    env, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use tempfile::{Builder as TempDirBuilder, TempDir};

use dirs::cache_dir;

use super::{
    error::BuildError,
    manifest::{CacheMode, CachePath, CachePolicy},
};

const CONTAINER_ROOT: &str = "/workspace";
const CACHE_ENV_VAR: &str = "ZMK_LAYOUT_CACHE_DIR";

/// Manages ephemeral workspaces for firmware builds.
#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    cache_root: PathBuf,
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceManager {
    pub fn new() -> Self {
        let cache_root = default_cache_root();
        Self { cache_root }
    }

    pub fn with_cache_root(root: impl Into<PathBuf>) -> Self {
        Self {
            cache_root: root.into(),
        }
    }

    pub fn create_workspace(
        &self,
        toolchain_id: &str,
        cache: &CachePolicy,
        disable_cache: bool,
    ) -> Result<WorkspaceHandle, BuildError> {
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

        let cache_config = WorkspaceCache::new(
            self.cache_root.clone(),
            toolchain_id.to_string(),
            cache,
            disable_cache,
        );

        let handle = WorkspaceHandle {
            root: dir,
            layout_dir,
            artifacts_dir,
            config_dir,
            app_dir,
            build_dir,
            cache: cache_config,
        };

        handle.hydrate_caches()?;
        Ok(handle)
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
    cache: WorkspaceCache,
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

    pub fn persist_caches(&self) -> Result<(), BuildError> {
        self.cache.persist(self)
    }

    fn hydrate_caches(&self) -> Result<(), BuildError> {
        self.cache.hydrate(self)
    }
}

#[derive(Debug, Clone)]
struct WorkspaceCache {
    cache_root: PathBuf,
    toolchain_id: String,
    workspace_mode: CacheMode,
    build_mode: CacheMode,
    extra_paths: Vec<CachePath>,
}

impl WorkspaceCache {
    fn new(
        cache_root: PathBuf,
        toolchain_id: String,
        policy: &CachePolicy,
        disable_cache: bool,
    ) -> Self {
        let mut workspace_mode = policy.workspace;
        let mut build_mode = policy.build;
        let mut extra_paths = policy.extra_paths.clone();
        if disable_cache {
            workspace_mode = CacheMode::Disabled;
            build_mode = CacheMode::Disabled;
            for path in &mut extra_paths {
                path.mode = CacheMode::Disabled;
            }
        }
        Self {
            cache_root,
            toolchain_id,
            workspace_mode,
            build_mode,
            extra_paths,
        }
    }

    fn hydrate(&self, workspace: &WorkspaceHandle) -> Result<(), BuildError> {
        if self.workspace_mode != CacheMode::Disabled {
            copy_from_cache(&self.workspace_cache_dir().join("app"), workspace.app_dir())?;
            copy_from_cache(
                &self.workspace_cache_dir().join("config"),
                workspace.config_dir(),
            )?;
        }
        if self.build_mode != CacheMode::Disabled {
            copy_from_cache(&self.build_cache_dir(), workspace.build_dir())?;
        }
        for entry in &self.extra_paths {
            if entry.mode == CacheMode::Disabled {
                continue;
            }
            let target = workspace.root().join(&entry.relative);
            copy_from_cache(&self.extra_cache_dir(&entry.relative), &target)?;
        }
        Ok(())
    }

    fn persist(&self, workspace: &WorkspaceHandle) -> Result<(), BuildError> {
        if self.workspace_mode == CacheMode::ReadWrite {
            copy_to_cache(workspace.app_dir(), &self.workspace_cache_dir().join("app"))?;
            copy_to_cache(
                workspace.config_dir(),
                &self.workspace_cache_dir().join("config"),
            )?;
        }
        if self.build_mode == CacheMode::ReadWrite {
            copy_to_cache(workspace.build_dir(), &self.build_cache_dir())?;
        }
        for entry in &self.extra_paths {
            if entry.mode != CacheMode::ReadWrite {
                continue;
            }
            let source = workspace.root().join(&entry.relative);
            if source.exists() {
                copy_to_cache(&source, &self.extra_cache_dir(&entry.relative))?;
            }
        }
        Ok(())
    }

    fn cache_root(&self) -> PathBuf {
        self.cache_root.join("firmware").join(&self.toolchain_id)
    }

    fn workspace_cache_dir(&self) -> PathBuf {
        self.cache_root().join("workspace")
    }

    fn build_cache_dir(&self) -> PathBuf {
        self.cache_root().join("build")
    }

    fn extra_cache_dir(&self, relative: &Path) -> PathBuf {
        self.cache_root()
            .join("extra")
            .join(sanitize_relative(relative))
    }
}

fn sanitize_relative(relative: &Path) -> String {
    let value = relative.to_string_lossy();
    let mut sanitized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }
    if sanitized.is_empty() {
        sanitized.push_str("root");
    }
    let mut hasher = DefaultHasher::new();
    relative.hash(&mut hasher);
    sanitized.push('_');
    sanitized.push_str(&format!("{:x}", hasher.finish()));
    sanitized
}

fn copy_from_cache(src: &Path, dest: &Path) -> Result<(), BuildError> {
    if !src.exists() {
        return Ok(());
    }
    clear_path(dest)?;
    if src.is_file() {
        copy_file(src, dest)
    } else {
        copy_dir_recursive(src, dest)
    }
}

fn copy_to_cache(src: &Path, dest: &Path) -> Result<(), BuildError> {
    if !src.exists() {
        return Ok(());
    }
    clear_path(dest)?;
    if src.is_file() {
        copy_file(src, dest)
    } else {
        copy_dir_recursive(src, dest)
    }
}

fn clear_path(path: &Path) -> Result<(), BuildError> {
    if path.exists() {
        if path.is_file() {
            fs::remove_file(path).map_err(BuildError::Io)?;
        } else {
            fs::remove_dir_all(path).map_err(BuildError::Io)?;
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), BuildError> {
    fs::create_dir_all(dest).map_err(BuildError::Io)?;
    for entry in fs::read_dir(src).map_err(BuildError::Io)? {
        let entry = entry.map_err(BuildError::Io)?;
        let ty = entry.file_type().map_err(BuildError::Io)?;
        let target = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            copy_file(&entry.path(), &target)?;
        }
    }
    Ok(())
}

fn copy_file(src: &Path, dest: &Path) -> Result<(), BuildError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(BuildError::Io)?;
    }
    fs::copy(src, dest).map_err(BuildError::Io)?;
    Ok(())
}

fn default_cache_root() -> PathBuf {
    if let Ok(path) = env::var(CACHE_ENV_VAR) {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    cache_dir()
        .unwrap_or_else(|| env::temp_dir())
        .join("zmk-layout")
}
