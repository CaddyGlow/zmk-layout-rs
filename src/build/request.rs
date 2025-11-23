//! Build request builder + layout source definitions.

use std::{collections::BTreeMap, fmt, path::PathBuf, sync::Arc};

use serde_json::Value as JsonValue;
use thiserror::Error;

use crate::{adapters::AdapterPipeline, dts::DtsDocument, profiles::KeyboardProfileDoc};

use super::{
    error::BuildError,
    manifest::{FirmwareManifest, KeyboardProfile},
    progress::{NoopProgressReporter, ProgressReporter},
};

/// Source input describing the layout to build.
#[derive(Debug, Clone)]
pub enum LayoutSource {
    JsonPath(PathBuf),
    JsonValue(JsonValue),
    Document(DtsDocument),
    Pipeline(AdapterPipeline),
    Files {
        keymap: PathBuf,
        extra: Option<PathBuf>,
    },
}

/// Target reference selected for a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildTargetRef {
    pub id: String,
}

/// Normalized firmware build request consumed by [`FirmwareBuilder`](crate::build::FirmwareBuilder).
pub struct BuildRequest {
    pub keyboard_id: String,
    pub toolchain_id: Option<String>,
    pub targets: Vec<BuildTargetRef>,
    pub layout: LayoutSource,
    pub output_dir: PathBuf,
    pub extra_env: BTreeMap<String, String>,
    pub disable_cache: bool,
    pub manifest: Arc<FirmwareManifest>,
    pub progress: Arc<dyn ProgressReporter>,
}

impl fmt::Debug for BuildRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildRequest")
            .field("keyboard_id", &self.keyboard_id)
            .field("toolchain_id", &self.toolchain_id)
            .field(
                "targets",
                &self
                    .targets
                    .iter()
                    .map(|t| t.id.clone())
                    .collect::<Vec<_>>(),
            )
            .field("output_dir", &self.output_dir)
            .field("disable_cache", &self.disable_cache)
            .field(
                "env_keys",
                &self.extra_env.keys().cloned().collect::<Vec<_>>(),
            )
            .field("manifest_version", &self.manifest.version)
            .finish_non_exhaustive()
    }
}

impl BuildRequest {
    pub fn keyboard_profile(&self) -> Option<&KeyboardProfile> {
        self.manifest.keyboards.get(&self.keyboard_id)
    }

    pub fn keyboard_profile_doc(&self) -> Option<&KeyboardProfileDoc> {
        self.keyboard_profile()
            .and_then(|profile| profile.profile.as_ref().map(|doc| &doc.document))
    }
}

/// Builder for [`BuildRequest`].
pub struct BuildRequestBuilder {
    manifest: Arc<FirmwareManifest>,
    keyboard_id: Option<String>,
    toolchain_id: Option<String>,
    targets: Vec<BuildTargetRef>,
    layout: Option<LayoutSource>,
    output_dir: Option<PathBuf>,
    extra_env: BTreeMap<String, String>,
    disable_cache: bool,
    progress: Option<Arc<dyn ProgressReporter>>,
}

impl BuildRequestBuilder {
    pub(crate) fn new(manifest: Arc<FirmwareManifest>) -> Self {
        Self {
            manifest,
            keyboard_id: None,
            toolchain_id: None,
            targets: Vec::new(),
            layout: None,
            output_dir: None,
            extra_env: BTreeMap::new(),
            disable_cache: false,
            progress: None,
        }
    }

    pub fn keyboard(mut self, id: impl Into<String>) -> Self {
        self.keyboard_id = Some(id.into());
        self
    }

    pub fn toolchain(mut self, id: impl Into<String>) -> Self {
        self.toolchain_id = Some(id.into());
        self
    }

    pub fn target(mut self, id: impl Into<String>) -> Self {
        self.targets.push(BuildTargetRef { id: id.into() });
        self
    }

    pub fn layout_json_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.layout = Some(LayoutSource::JsonPath(path.into()));
        self
    }

    pub fn layout_json_value(mut self, value: JsonValue) -> Self {
        self.layout = Some(LayoutSource::JsonValue(value));
        self
    }

    pub fn layout_document(mut self, document: DtsDocument) -> Self {
        self.layout = Some(LayoutSource::Document(document));
        self
    }

    pub fn layout_via_pipeline(mut self, pipeline: AdapterPipeline) -> Self {
        self.layout = Some(LayoutSource::Pipeline(pipeline));
        self
    }

    pub fn layout_files(mut self, keymap: impl Into<PathBuf>, extra: Option<PathBuf>) -> Self {
        self.layout = Some(LayoutSource::Files {
            keymap: keymap.into(),
            extra,
        });
        self
    }

    pub fn output_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.output_dir = Some(path.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_env.insert(key.into(), value.into());
        self
    }

    pub fn disable_cache(mut self, flag: bool) -> Self {
        self.disable_cache = flag;
        self
    }

    pub fn progress(mut self, reporter: Arc<dyn ProgressReporter>) -> Self {
        self.progress = Some(reporter);
        self
    }

    pub fn build(self) -> Result<BuildRequest, BuildRequestError> {
        let manifest = self.manifest.clone();
        let keyboard_id = self.keyboard_id.ok_or(BuildRequestError::MissingKeyboard)?;
        let keyboard = manifest
            .keyboards
            .get(&keyboard_id)
            .ok_or_else(|| BuildRequestError::UnknownKeyboard(keyboard_id.clone()))?;

        let toolchain_id = if let Some(id) = self.toolchain_id {
            if !manifest.toolchains.contains_key(&id) {
                return Err(BuildRequestError::UnknownToolchain(id));
            }
            Some(id)
        } else {
            None
        };

        let targets = if self.targets.is_empty() {
            keyboard
                .targets
                .iter()
                .map(|target| BuildTargetRef {
                    id: target.id.clone(),
                })
                .collect()
        } else {
            validate_targets(keyboard, &self.targets)?
        };

        let layout = self.layout.ok_or(BuildRequestError::MissingLayout)?;
        let output_dir = self
            .output_dir
            .ok_or(BuildRequestError::MissingOutputDirectory)?;
        let progress = self
            .progress
            .unwrap_or_else(|| Arc::new(NoopProgressReporter::new()));

        Ok(BuildRequest {
            keyboard_id,
            toolchain_id,
            targets,
            layout,
            output_dir,
            extra_env: self.extra_env,
            disable_cache: self.disable_cache,
            manifest,
            progress,
        })
    }
}

fn validate_targets(
    keyboard: &KeyboardProfile,
    requested: &[BuildTargetRef],
) -> Result<Vec<BuildTargetRef>, BuildRequestError> {
    let mut resolved = Vec::with_capacity(requested.len());
    for target in requested {
        if keyboard.targets.iter().any(|t| t.id == target.id) {
            resolved.push(target.clone());
        } else {
            return Err(BuildRequestError::UnknownTarget {
                keyboard: keyboard.id.clone(),
                target: target.id.clone(),
            });
        }
    }
    Ok(resolved)
}

/// Errors surfaced by the [`BuildRequestBuilder`].
#[derive(Debug, Error)]
pub enum BuildRequestError {
    #[error("keyboard id is required")]
    MissingKeyboard,
    #[error("keyboard `{0}` is not present in the manifest")]
    UnknownKeyboard(String),
    #[error("toolchain `{0}` is not present in the manifest")]
    UnknownToolchain(String),
    #[error("output directory must be provided")]
    MissingOutputDirectory,
    #[error("layout input must be provided")]
    MissingLayout,
    #[error("keyboard `{keyboard}` does not define target `{target}`")]
    UnknownTarget { keyboard: String, target: String },
    #[error("failed to validate request: {0}")]
    Build(String),
    #[error(transparent)]
    BuildError(#[from] BuildError),
}
