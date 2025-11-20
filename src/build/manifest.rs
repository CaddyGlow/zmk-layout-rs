//! Manifest and profile definitions for the firmware builder pipeline.

use crate::{
    profiles::{EmbeddedFirmwareProfiles, KeyboardProfileDoc, ProfileError},
    tasks::MetadataMap,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Fully parsed firmware manifest describing toolchains + keyboards.
#[derive(Debug, Clone)]
pub struct FirmwareManifest {
    pub version: u32,
    pub toolchains: HashMap<String, ToolchainProfile>,
    pub keyboards: HashMap<String, KeyboardProfile>,
}

impl FirmwareManifest {
    /// Load a manifest from raw TOML text.
    pub fn from_toml_str(input: &str) -> Result<Self, ManifestError> {
        let raw: RawFirmwareManifest = toml::from_str(input).map_err(ManifestError::Parse)?;
        Self::from_raw(raw, None)
    }

    /// Load a manifest from a file path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path_ref = path.as_ref();
        let contents = fs::read_to_string(path_ref).map_err(|source| ManifestError::ReadFile {
            path: path_ref.to_path_buf(),
            source,
        })?;
        let raw: RawFirmwareManifest = toml::from_str(&contents).map_err(ManifestError::Parse)?;
        Self::from_raw(raw, path_ref.parent())
    }

    /// Load a firmware manifest by name, checking filesystem first, then embedded profiles.
    ///
    /// This allows external files to override embedded manifests.
    /// The name should be just the manifest name without path or extension (e.g., "glove80").
    ///
    /// Search order:
    /// 1. `firmware_profiles/{name}.toml` in filesystem
    /// 2. Embedded manifest `{name}.toml`
    pub fn load(name: &str) -> Result<Self, ManifestError> {
        let filename = format!("{}.toml", name);
        let fs_path = PathBuf::from("firmware_profiles").join(&filename);

        // Try filesystem first (allows override)
        if fs_path.exists() {
            return Self::from_file(&fs_path);
        }

        // Fall back to embedded manifest
        let embedded = EmbeddedFirmwareProfiles::get(&filename)
            .ok_or_else(|| ManifestError::NotFound(name.to_string()))?;
        let contents = std::str::from_utf8(embedded.data.as_ref())
            .map_err(|_| ManifestError::InvalidUtf8)?;
        Self::from_toml_str(contents)
    }

    /// List all available firmware manifests (both embedded and filesystem).
    pub fn list_available() -> Vec<String> {
        let mut manifests = std::collections::BTreeSet::new();

        // Add embedded manifests
        for file in EmbeddedFirmwareProfiles::iter() {
            if let Some(name) = file.as_ref().strip_suffix(".toml") {
                manifests.insert(name.to_string());
            }
        }

        // Add filesystem manifests (may override embedded)
        if let Ok(entries) = fs::read_dir("firmware_profiles") {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    if entry.path().extension().and_then(|s| s.to_str()) == Some("toml") {
                        manifests.insert(name.to_string());
                    }
                }
            }
        }

        manifests.into_iter().collect()
    }

    fn from_raw(
        raw: RawFirmwareManifest,
        manifest_root: Option<&Path>,
    ) -> Result<Self, ManifestError> {
        if raw.toolchains.is_empty() {
            return Err(ManifestError::NoToolchains);
        }
        if raw.keyboards.is_empty() {
            return Err(ManifestError::NoKeyboards);
        }

        let mut toolchains = HashMap::with_capacity(raw.toolchains.len());
        for (id, profile) in raw.toolchains {
            let profile = profile.into_profile(id.clone());
            toolchains.insert(id, profile);
        }

        let mut keyboards = HashMap::with_capacity(raw.keyboards.len());
        for (id, profile) in raw.keyboards {
            let profile = profile.into_profile(id.clone(), &toolchains, manifest_root)?;
            keyboards.insert(id, profile);
        }

        Ok(FirmwareManifest {
            version: raw.version,
            toolchains,
            keyboards,
        })
    }
}

/// Toolchain definition loaded from the manifest.
#[derive(Debug, Clone)]
pub struct ToolchainProfile {
    pub id: String,
    pub kind: ToolchainKind,
    pub image: String,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub env: BTreeMap<String, String>,
    pub cache: CachePolicy,
}

/// Supported toolchain variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolchainKind {
    ZmkConfig,
    Moergo,
}

/// Cache policy used by toolchains and targets.
#[derive(Debug, Clone)]
pub struct CachePolicy {
    pub workspace: CacheMode,
    pub build: CacheMode,
    pub extra_paths: Vec<CachePath>,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            workspace: CacheMode::Disabled,
            build: CacheMode::Disabled,
            extra_paths: Vec::new(),
        }
    }
}

/// Additional workspace-relative path to hydrate/store in caches.
#[derive(Debug, Clone)]
pub struct CachePath {
    pub relative: PathBuf,
    pub mode: CacheMode,
}

/// Cache enablement options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    Disabled,
    ReadOnly,
    ReadWrite,
}

impl Default for CacheMode {
    fn default() -> Self {
        CacheMode::Disabled
    }
}

/// Keyboard definition referencing build targets.
#[derive(Debug, Clone)]
pub struct KeyboardProfile {
    pub id: String,
    pub default_toolchain: String,
    pub targets: Vec<BuildTarget>,
    pub metadata: MetadataMap,
    pub profile: Option<KeyboardProfileDocument>,
}

/// Loaded keyboard profile reference parsed from metadata.profile.
#[derive(Debug, Clone)]
pub struct KeyboardProfileDocument {
    pub path: PathBuf,
    pub document: KeyboardProfileDoc,
}

/// Target entry representing board/shield pairs.
#[derive(Debug, Clone)]
pub struct BuildTarget {
    pub id: String,
    pub board: String,
    pub shield: Option<String>,
    pub cmake_defs: BTreeMap<String, String>,
    pub variant: Option<String>,
    pub comment: Option<String>,
    pub repo_override: Option<String>,
    pub branch_override: Option<String>,
    pub toolchain_overrides: HashMap<String, ToolchainOverride>,
}

/// Per-target override configuration for concrete toolchains.
#[derive(Debug, Clone)]
pub struct ToolchainOverride {
    pub image: Option<String>,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RawFirmwareManifest {
    #[serde(default)]
    version: u32,
    toolchains: HashMap<String, RawToolchainProfile>,
    keyboards: HashMap<String, RawKeyboardProfile>,
}

#[derive(Debug, Deserialize)]
struct RawToolchainProfile {
    kind: ToolchainKind,
    image: String,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    cache: Option<RawCachePolicy>,
}

impl RawToolchainProfile {
    fn into_profile(self, id: String) -> ToolchainProfile {
        ToolchainProfile {
            id,
            kind: self.kind,
            image: self.image,
            repository: self.repository,
            branch: self.branch,
            env: self.env.unwrap_or_default(),
            cache: self.cache.unwrap_or_default().into_policy(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct RawCachePolicy {
    #[serde(default)]
    workspace: Option<CacheMode>,
    #[serde(default)]
    build: Option<CacheMode>,
    #[serde(default)]
    extra_paths: Vec<RawCachePath>,
}

impl RawCachePolicy {
    fn into_policy(self) -> CachePolicy {
        let mut policy = CachePolicy::default();
        if let Some(mode) = self.workspace {
            policy.workspace = mode;
        }
        if let Some(mode) = self.build {
            policy.build = mode;
        }
        let workspace_default = policy.workspace;
        let build_default = policy.build;
        policy.extra_paths = self
            .extra_paths
            .into_iter()
            .map(|entry| entry.into_path(workspace_default, build_default))
            .collect();
        policy
    }
}

#[derive(Debug, Deserialize)]
struct RawCachePath {
    relative: PathBuf,
    #[serde(default)]
    mode: Option<CacheMode>,
}

impl RawCachePath {
    fn into_path(self, workspace_default: CacheMode, build_default: CacheMode) -> CachePath {
        let fallback = match (workspace_default, build_default) {
            (CacheMode::Disabled, other) => other,
            (mode, _) => mode,
        };
        CachePath {
            relative: self.relative,
            mode: self.mode.unwrap_or(fallback),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawKeyboardProfile {
    default_toolchain: String,
    #[serde(default)]
    targets: Vec<RawBuildTarget>,
    #[serde(default)]
    metadata: MetadataMap,
}

impl RawKeyboardProfile {
    fn into_profile(
        self,
        id: String,
        toolchains: &HashMap<String, ToolchainProfile>,
        manifest_root: Option<&Path>,
    ) -> Result<KeyboardProfile, ManifestError> {
        if !toolchains.contains_key(&self.default_toolchain) {
            return Err(ManifestError::MissingDefaultToolchain(
                id.clone(),
                self.default_toolchain.clone(),
            ));
        }

        let mut seen_targets = HashSet::new();
        let mut targets = Vec::with_capacity(self.targets.len());
        for raw_target in self.targets {
            let target_id = raw_target.target_id(&id)?;
            if !seen_targets.insert(target_id.clone()) {
                return Err(ManifestError::DuplicateTarget {
                    keyboard: id.clone(),
                    target: target_id,
                });
            }
            let target = raw_target.into_target(&id, &target_id, toolchains)?;
            targets.push(target);
        }
        let profile = load_keyboard_profile(&id, &self.metadata, manifest_root)?;

        Ok(KeyboardProfile {
            id,
            default_toolchain: self.default_toolchain,
            targets,
            metadata: self.metadata,
            profile,
        })
    }
}

fn load_keyboard_profile(
    keyboard: &str,
    metadata: &MetadataMap,
    manifest_root: Option<&Path>,
) -> Result<Option<KeyboardProfileDocument>, ManifestError> {
    let Some(value) = metadata.get("profile") else {
        return Ok(None);
    };
    let path_value =
        value
            .as_str()
            .ok_or_else(|| ManifestError::InvalidKeyboardProfileReference {
                keyboard: keyboard.to_string(),
            })?;
    let trimmed = path_value.trim();
    if trimmed.is_empty() {
        return Err(ManifestError::InvalidKeyboardProfileReference {
            keyboard: keyboard.to_string(),
        });
    }
    if is_legacy_profile_reference(trimmed) {
        eprintln!(
            "warning: keyboard `{}` metadata.profile points to legacy file `{}`; skipping profile hydration",
            keyboard, trimmed
        );
        return Ok(None);
    }

    // Check if this is a profile name (no path separators) - use embedded/filesystem load
    if !trimmed.contains('/') && !trimmed.contains('\\') && !trimmed.ends_with(".toml") {
        match KeyboardProfileDoc::load(trimmed) {
            Ok(document) => {
                return Ok(Some(KeyboardProfileDocument {
                    path: PathBuf::from(format!("keyboard_profiles/{}.toml", trimmed)),
                    document,
                }));
            }
            Err(ProfileError::NotFound(_)) => {
                // Fall through to path-based loading
            }
            Err(err) => {
                return Err(ManifestError::KeyboardProfileLoad {
                    keyboard: keyboard.to_string(),
                    path: PathBuf::from(trimmed),
                    source: err,
                });
            }
        }
    }

    // Path-based loading (original behavior)
    let candidates = keyboard_profile_candidates(trimmed, manifest_root);
    let mut last_read_error: Option<(PathBuf, ProfileError)> = None;
    for candidate in candidates {
        match KeyboardProfileDoc::from_file(&candidate) {
            Ok(document) => {
                return Ok(Some(KeyboardProfileDocument {
                    path: candidate,
                    document,
                }));
            }
            Err(err @ ProfileError::ReadFile { .. }) => {
                last_read_error = Some((candidate, err));
                continue;
            }
            Err(err) => {
                return Err(ManifestError::KeyboardProfileLoad {
                    keyboard: keyboard.to_string(),
                    path: candidate,
                    source: err,
                });
            }
        }
    }
    if let Some((path, err)) = last_read_error {
        return Err(ManifestError::KeyboardProfileLoad {
            keyboard: keyboard.to_string(),
            path,
            source: err,
        });
    }
    Ok(None)
}

fn keyboard_profile_candidates(path: &str, manifest_root: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let input = PathBuf::from(path);
    candidates.push(input.clone());
    if !input.is_absolute() {
        if let Some(root) = manifest_root {
            let joined = root.join(&input);
            if joined != input {
                candidates.push(joined);
            }
        }
    }
    candidates
}

fn is_legacy_profile_reference(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".yaml") || lower.ends_with(".yml")
}

#[derive(Debug, Deserialize)]
struct RawBuildTarget {
    id: String,
    board: String,
    #[serde(default)]
    shield: Option<String>,
    #[serde(default)]
    cmake_defs: Option<BTreeMap<String, String>>,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    repo_override: Option<String>,
    #[serde(default)]
    branch_override: Option<String>,
    #[serde(default)]
    toolchain_overrides: Option<HashMap<String, RawToolchainOverride>>,
}

impl RawBuildTarget {
    fn target_id(&self, keyboard: &str) -> Result<String, ManifestError> {
        let trimmed = self.id.trim();
        if trimmed.is_empty() {
            return Err(ManifestError::MissingTargetId {
                keyboard: keyboard.to_string(),
            });
        }
        Ok(trimmed.to_string())
    }

    fn into_target(
        self,
        keyboard: &str,
        target_id: &str,
        toolchains: &HashMap<String, ToolchainProfile>,
    ) -> Result<BuildTarget, ManifestError> {
        let mut overrides = HashMap::new();
        if let Some(map) = self.toolchain_overrides {
            for (toolchain_id, raw_override) in map {
                if !toolchains.contains_key(&toolchain_id) {
                    return Err(ManifestError::UnknownToolchainOverride {
                        keyboard: keyboard.to_string(),
                        target: target_id.to_string(),
                        toolchain: toolchain_id,
                    });
                }
                overrides.insert(toolchain_id, raw_override.into_override());
            }
        }

        Ok(BuildTarget {
            id: target_id.to_string(),
            board: self.board,
            shield: self.shield,
            cmake_defs: self.cmake_defs.unwrap_or_default(),
            variant: self.variant,
            comment: self.comment,
            repo_override: self.repo_override,
            branch_override: self.branch_override,
            toolchain_overrides: overrides,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawToolchainOverride {
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
}

impl RawToolchainOverride {
    fn into_override(self) -> ToolchainOverride {
        ToolchainOverride {
            image: self.image,
            repository: self.repository,
            branch: self.branch,
            env: self.env.unwrap_or_default(),
        }
    }
}

/// Errors raised while parsing manifests.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("failed to read manifest {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse manifest: {0}")]
    Parse(toml::de::Error),
    #[error("toolchains cannot be empty")]
    NoToolchains,
    #[error("keyboards cannot be empty")]
    NoKeyboards,
    #[error("keyboard `{0}` is missing default toolchain `{1}`")]
    MissingDefaultToolchain(String, String),
    #[error("keyboard `{keyboard}` defines duplicate target `{target}`")]
    DuplicateTarget { keyboard: String, target: String },
    #[error("keyboard `{keyboard}` target `{target}` references unknown toolchain `{toolchain}`")]
    UnknownToolchainOverride {
        keyboard: String,
        target: String,
        toolchain: String,
    },
    #[error("keyboard `{keyboard}` has a target with an empty id")]
    MissingTargetId { keyboard: String },
    #[error("keyboard `{keyboard}` metadata.profile must be a non-empty string")]
    InvalidKeyboardProfileReference { keyboard: String },
    #[error("keyboard `{keyboard}` profile `{path}` could not be loaded: {source}")]
    KeyboardProfileLoad {
        keyboard: String,
        path: PathBuf,
        #[source]
        source: ProfileError,
    },
    #[error("firmware manifest `{0}` not found in embedded profiles or filesystem")]
    NotFound(String),
    #[error("embedded resource is not valid UTF-8")]
    InvalidUtf8,
}
