//! High-level firmware builder facade used by the CLI/library.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use serde::Serialize;

use super::{
    docker::DockerBackend,
    error::BuildError,
    kconfig::KconfigResolver,
    layout::LayoutStager,
    logs::LogFile,
    manifest::{BuildTarget, FirmwareManifest, KeyboardProfile, ToolchainProfile},
    progress::ProgressReporter,
    request::{BuildRequest, BuildRequestBuilder},
    toolchain::{create_toolchain, BuildContext},
    workspace::WorkspaceManager,
};

/// Firmware build coordinator – later phases will wire toolchains here.
pub struct FirmwareBuilder {
    manifest: Arc<FirmwareManifest>,
    docker: Box<dyn DockerBackend>,
    workspace_manager: WorkspaceManager,
}

impl FirmwareBuilder {
    pub fn new(manifest: FirmwareManifest, docker: Box<dyn DockerBackend>) -> Self {
        Self {
            manifest: Arc::new(manifest),
            docker,
            workspace_manager: WorkspaceManager::new(),
        }
    }

    pub fn with_workspace_manager(mut self, manager: WorkspaceManager) -> Self {
        self.workspace_manager = manager;
        self
    }

    pub fn builder(&self) -> BuildRequestBuilder {
        BuildRequestBuilder::new(self.manifest.clone())
    }

    pub fn docker(&self) -> &dyn DockerBackend {
        self.docker.as_ref()
    }

    pub fn manifest(&self) -> &FirmwareManifest {
        &self.manifest
    }

    pub fn build(&self, request: BuildRequest) -> Result<BuildReport, BuildError> {
        let keyboard = self
            .manifest
            .keyboards
            .get(&request.keyboard_id)
            .ok_or_else(|| BuildError::UnknownKeyboard(request.keyboard_id.clone()))?;
        let toolchain_id = request
            .toolchain_id
            .clone()
            .unwrap_or_else(|| keyboard.default_toolchain.clone());
        let profile = self
            .manifest
            .toolchains
            .get(&toolchain_id)
            .ok_or_else(|| BuildError::UnknownToolchain(toolchain_id.clone()))?;
        let toolchain = create_toolchain(profile)?;

        fs::create_dir_all(&request.output_dir).map_err(BuildError::Io)?;
        let log_filename = format!(
            "build-{}-{}.log",
            sanitize_slug(&keyboard.id),
            sanitize_slug(&profile.id)
        );
        let log_path = request.output_dir.join(log_filename);
        let log_file = LogFile::create(&log_path)?;
        let build_started = Instant::now();

        let workspace = self.workspace_manager.create_workspace(
            &profile.id,
            &profile.cache,
            request.disable_cache,
        )?;
        let stager = LayoutStager::new();
        let mut layout = stager.stage(
            &request.layout,
            request.keyboard_profile_document(),
            &workspace,
        )?;

        // -- Kconfig resolution --
        let kconfig_resolution = {
            let resolver = if let Some(user_file) = &request.kconfig_file {
                KconfigResolver::new_from_file(user_file.clone(), request.kconfig_defs.clone())
            } else {
                KconfigResolver::new_generated(request.kconfig_defs.clone())
            };

            // Gather profile kconfig map from vendor
            let profile_kconfig_map = request
                .keyboard_profile_doc()
                .and_then(|doc| {
                    crate::profiles::load_vendor_kconfig_options(&doc.metadata.vendor)
                });

            // Gather hardware defaults
            let hardware_defaults = request.keyboard_profile_doc().and_then(|doc| {
                let kconfig = &doc.hardware.build_defaults.kconfig;
                if kconfig.is_empty() {
                    None
                } else {
                    Some(
                        kconfig
                            .iter()
                            .map(|(k, v)| (k.clone(), toml_value_to_string(v)))
                            .collect::<BTreeMap<String, String>>(),
                    )
                }
            });

            // Gather firmware version kconfig
            let firmware_kconfig = request.keyboard_profile_doc().and_then(|doc| {
                let version = doc.firmware.versions.get(&doc.firmware.default)?;
                let props = &version.properties;
                let kconfig = props.get("kconfig")?.as_table()?;
                if kconfig.is_empty() {
                    return None;
                }
                Some(
                    kconfig
                        .iter()
                        .map(|(k, v)| (k.clone(), toml_value_to_string(v)))
                        .collect::<BTreeMap<String, String>>(),
                )
            });

            let json_params = request.json_config_params.as_deref();

            resolver.resolve(
                workspace.layout_dir(),
                profile_kconfig_map.as_ref(),
                hardware_defaults.as_ref(),
                firmware_kconfig.as_ref(),
                json_params,
            )?
        };

        // Set config artifact from kconfig resolution
        if let Some(config_path) = kconfig_resolution.config_path {
            layout.config = Some(config_path);
        }

        for warning in &kconfig_resolution.warnings {
            request.progress.log(super::LogLevel::Warn, warning);
        }

        let ctx = BuildContext {
            manifest: &self.manifest,
            keyboard,
            profile,
            request: &request,
            workspace: &workspace,
            layout: &layout,
            progress: request.progress.clone(),
            log_file: Some(log_file.clone()),
        };

        let mut report = BuildReport::default();
        report.success = true;
        report.logs_path = Some(log_path.clone());
        let total_targets = request.targets.len();
        let checkpoint_id = format!(
            "firmware-{}-{}",
            keyboard.id.replace(' ', "-"),
            toolchain_id.as_str()
        );
        if total_targets > 0 {
            let plural = if total_targets == 1 { "" } else { "s" };
            request.progress.start_checkpoint(
                &checkpoint_id,
                &format!(
                    "Building {} via {} ({} target{})",
                    keyboard.id, profile.id, total_targets, plural
                ),
            );
        }
        let progress_total = total_targets as u32;
        let mut built_targets = Vec::with_capacity(total_targets);
        for (index, target_ref) in request.targets.iter().enumerate() {
            if total_targets > 0 {
                request.progress.update_progress(
                    index as u32,
                    progress_total,
                    &format!("building target {}", target_ref.id),
                );
            }
            if let Some(logger) = &ctx.log_file {
                logger.append("builder", &format!("starting target {}", target_ref.id));
            }
            let target = match resolve_target(keyboard, &target_ref.id) {
                Ok(target) => target,
                Err(err) => {
                    if let Some(logger) = &ctx.log_file {
                        logger.append(
                            "builder",
                            &format!("failed to resolve target {}: {err}", target_ref.id),
                        );
                    }
                    if total_targets > 0 {
                        request.progress.fail_checkpoint(&checkpoint_id);
                    }
                    return Err(err);
                }
            };
            let result = match toolchain.build_target(&ctx, target, self.docker()) {
                Ok(result) => result,
                Err(err) => {
                    if let Some(logger) = &ctx.log_file {
                        logger.append("builder", &format!("target {} failed: {err}", target.id));
                    }
                    if total_targets > 0 {
                        request.progress.fail_checkpoint(&checkpoint_id);
                    }
                    return Err(err);
                }
            };
            for artifact in &result.artifacts {
                report
                    .artifacts
                    .per_target
                    .entry(target.id.clone())
                    .or_default()
                    .push(artifact.clone());
            }
            report.artifacts.files.extend(result.artifacts);
            built_targets.push(target.id.clone());
            if let Some(logger) = &ctx.log_file {
                logger.append("builder", &format!("completed target {}", target.id));
            }
            if total_targets > 0 {
                request.progress.update_progress(
                    (index as u32) + 1,
                    progress_total,
                    &format!("completed target {}", target.id),
                );
            }
        }

        if let Err(err) = workspace.persist_caches() {
            if total_targets > 0 {
                request.progress.fail_checkpoint(&checkpoint_id);
            }
            return Err(err);
        }

        if total_targets > 0 {
            request.progress.complete_checkpoint(&checkpoint_id);
        }

        populate_metadata(
            &mut report.metadata,
            keyboard,
            profile,
            &request,
            &built_targets,
            report.artifacts.files.len(),
        );
        let duration_ms = build_started.elapsed().as_millis();
        let info_path = write_build_info(
            &request,
            keyboard,
            profile,
            &report,
            report.logs_path.as_deref(),
            duration_ms,
        )?;
        report.build_info_path = Some(info_path);

        Ok(report)
    }
}

/// Build report returned by [`FirmwareBuilder::build`].
#[derive(Debug, Clone)]
pub struct BuildReport {
    pub success: bool,
    pub artifacts: ArtifactReport,
    pub logs_path: Option<PathBuf>,
    pub build_info_path: Option<PathBuf>,
    pub metadata: BuildMetadata,
}

impl Default for BuildReport {
    fn default() -> Self {
        Self {
            success: false,
            artifacts: ArtifactReport::default(),
            logs_path: None,
            build_info_path: None,
            metadata: BuildMetadata::default(),
        }
    }
}

/// Collection of build artifacts (.uf2, .bin, logs, etc.).
#[derive(Debug, Clone, Default)]
pub struct ArtifactReport {
    pub files: Vec<PathBuf>,
    pub per_target: BTreeMap<String, Vec<PathBuf>>,
}

/// Extra metadata captured during a build (toolchain info, timings, etc.).
#[derive(Debug, Clone, Default)]
pub struct BuildMetadata {
    pub entries: BTreeMap<String, String>,
}

/// Placeholder progress reporter used by the CLI.
pub struct CliProgressReporter;

impl ProgressReporter for CliProgressReporter {
    fn log(&self, level: super::LogLevel, message: &str) {
        eprintln!("[{level:?}] {message}");
    }

    fn start_checkpoint(&self, id: &str, message: &str) {
        eprintln!("[START] {id}: {message}");
    }

    fn complete_checkpoint(&self, id: &str) {
        eprintln!("[DONE ] {id}");
    }

    fn fail_checkpoint(&self, id: &str) {
        eprintln!("[FAIL ] {id}");
    }

    fn update_progress(&self, current: u32, total: u32, status: &str) {
        if total == 0 {
            eprintln!("[PROG ] {status}");
        } else {
            eprintln!("[PROG ] {current}/{total} {status}");
        }
    }
}

fn populate_metadata(
    metadata: &mut BuildMetadata,
    keyboard: &KeyboardProfile,
    profile: &ToolchainProfile,
    request: &BuildRequest,
    completed_targets: &[String],
    artifact_count: usize,
) {
    metadata
        .entries
        .insert("keyboard".into(), keyboard.id.clone());
    metadata
        .entries
        .insert("toolchain".into(), profile.id.clone());
    metadata
        .entries
        .insert("toolchain.kind".into(), format!("{:?}", profile.kind));
    metadata.entries.insert(
        "manifest.version".into(),
        request.manifest.version.to_string(),
    );
    metadata
        .entries
        .insert("targets.count".into(), request.targets.len().to_string());
    if !request.targets.is_empty() {
        let selected = request
            .targets
            .iter()
            .map(|target| target.id.clone())
            .collect::<Vec<_>>()
            .join(",");
        metadata.entries.insert("targets.selected".into(), selected);
    }
    if !completed_targets.is_empty() {
        metadata
            .entries
            .insert("targets.completed".into(), completed_targets.join(","));
    }
    metadata
        .entries
        .insert("artifacts.count".into(), artifact_count.to_string());
}

fn write_build_info(
    request: &BuildRequest,
    keyboard: &KeyboardProfile,
    profile: &ToolchainProfile,
    report: &BuildReport,
    log_path: Option<&Path>,
    duration_ms: u128,
) -> Result<PathBuf, BuildError> {
    let filename = format!(
        "build-info-{}-{}.json",
        sanitize_slug(&keyboard.id),
        sanitize_slug(&profile.id)
    );
    let info_path = request.output_dir.join(filename);
    let targets = request
        .targets
        .iter()
        .map(|target| BuildInfoTarget {
            id: target.id.clone(),
            artifacts: report
                .artifacts
                .per_target
                .get(&target.id)
                .map(|paths| paths.iter().map(|p| p.display().to_string()).collect())
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    let info = BuildInfo {
        keyboard: keyboard.id.clone(),
        toolchain: profile.id.clone(),
        toolchain_kind: format!("{:?}", profile.kind),
        success: report.success,
        duration_ms,
        log_file: log_path.map(|path| path.display().to_string()),
        artifacts: report
            .artifacts
            .files
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        metadata: report.metadata.entries.clone(),
        targets,
    };
    let data = serde_json::to_vec_pretty(&info)
        .map_err(|err| BuildError::InvalidRequest(format!("serialize build-info: {err}")))?;
    fs::write(&info_path, data).map_err(BuildError::Io)?;
    Ok(info_path)
}

#[derive(Serialize)]
struct BuildInfo {
    keyboard: String,
    toolchain: String,
    toolchain_kind: String,
    success: bool,
    duration_ms: u128,
    log_file: Option<String>,
    artifacts: Vec<String>,
    metadata: BTreeMap<String, String>,
    targets: Vec<BuildInfoTarget>,
}

#[derive(Serialize)]
struct BuildInfoTarget {
    id: String,
    artifacts: Vec<String>,
}

fn sanitize_slug(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    let mut last_was_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    if slug.is_empty() {
        slug.push_str("build");
    }
    slug.trim_matches('-').to_string()
}

fn resolve_target<'a>(
    keyboard: &'a KeyboardProfile,
    id: &str,
) -> Result<&'a BuildTarget, BuildError> {
    keyboard
        .targets
        .iter()
        .find(|target| target.id == id)
        .ok_or_else(|| BuildError::UnknownTarget {
            keyboard: keyboard.id.clone(),
            target: id.to_string(),
        })
}

fn toml_value_to_string(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => {
            if *b {
                "y".to_string()
            } else {
                "n".to_string()
            }
        }
        other => other.to_string(),
    }
}
