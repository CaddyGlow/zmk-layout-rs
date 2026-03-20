use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;
use crate::cli::{
    app::{DetectModeFlag, FirmwareBuildArgs, FirmwareDevicesArgs, FirmwareFlashArgs},
    error::CliError,
};
use zmk_layout_core::{
    adapters::{moergo, standard::AdapterLayout, AdapterPipeline},
    build::{
        BuildError, BuildReport, BuildRequest, BuildRequestBuilder, CliDockerBackend,
        CliProgressReporter, FirmwareBuilder, FirmwareManifest, LayoutSource,
    },
    flash::{
        build_flash_targets, default_sides, discover_devices, flash_target, render_device,
        render_flash_outcome, render_flash_warning, resolve_flash_source, DetectMode, FlashConfig,
    },
    io,
};

pub fn build(args: &FirmwareBuildArgs) -> Result<i32, CliError> {
    let manifest = load_manifest_flexible(&args.manifest)?;
    let builder = FirmwareBuilder::new(manifest, Box::new(CliDockerBackend::new()));
    let request = build_firmware_request(&builder, args)?;
    print_firmware_request(&request);
    if args.dry_run {
        return Ok(0);
    }
    let report = match builder.build(request) {
        Ok(report) => report,
        Err(BuildError::Cancelled) => {
            eprintln!("build cancelled by user");
            return Ok(130);
        }
        Err(err) => return Err(err.into()),
    };
    print_build_report(&report);
    if report.success {
        Ok(0)
    } else {
        Ok(2)
    }
}

pub fn flash(args: &FirmwareFlashArgs) -> Result<i32, CliError> {
    let manifest = load_manifest_flexible(&args.manifest)?;
    let keyboard = manifest
        .keyboards
        .get(&args.keyboard)
        .ok_or_else(|| CliError::UnknownKeyboard(args.keyboard.clone()))?;
    let profile = keyboard
        .profile
        .as_ref()
        .ok_or_else(|| CliError::MissingKeyboardProfile(args.keyboard.clone()))?;
    let side_flag = args.side.map(Into::into);
    let sides = default_sides(Some(&profile.document), side_flag);
    let mut config = profile
        .document
        .hardware
        .flash
        .first()
        .map(FlashConfig::from)
        .unwrap_or_default();
    if let Some(seconds) = args.mount_timeout {
        config.mount_timeout = Duration::from_secs(seconds);
    }
    if let Some(seconds) = args.copy_timeout {
        config.copy_timeout = Duration::from_secs(seconds);
    }
    if args.no_sync {
        config.sync_after_copy = false;
    }
    if let Some(mode) = args.detect {
        config.detect_mode = match mode {
            DetectModeFlag::Poll => DetectMode::Poll,
            DetectModeFlag::Events => DetectMode::Events,
        };
    }

    let source = resolve_flash_source(
        args.firmware.clone(),
        args.left.clone(),
        args.right.clone(),
        args.build_info.as_deref(),
        args.artifacts.as_deref(),
        &sides,
    )?;
    let targets = build_flash_targets(&profile.document, &sides);
    let mut seen_serials: HashSet<String> = HashSet::new();
    if targets.len() > 1 {
        eprintln!(
            "detected split keyboard; flashing {} sides sequentially",
            targets.len()
        );
    }
    for target in &targets {
        if targets.len() > 1 {
            eprintln!("-- prepare the {} half --", target.side);
        }
        let outcome = flash_target(&target, &source, args.device.as_deref(), &mut seen_serials)?;
        eprintln!("{}", render_flash_outcome(&outcome));
        for warning in outcome.warnings {
            eprintln!("{}", render_flash_warning(&warning));
        }
    }
    Ok(0)
}

pub fn devices(args: &FirmwareDevicesArgs) -> Result<i32, CliError> {
    let manifest = load_manifest_flexible(&args.manifest)?;
    let keyboard = manifest
        .keyboards
        .get(&args.keyboard)
        .ok_or_else(|| CliError::UnknownKeyboard(args.keyboard.clone()))?;
    let profile = keyboard
        .profile
        .as_ref()
        .ok_or_else(|| CliError::MissingKeyboardProfile(args.keyboard.clone()))?;
    let mut config = profile
        .document
        .hardware
        .flash
        .first()
        .map(FlashConfig::from)
        .unwrap_or_default();
    if args.all {
        config.device_query = None;
    }
    if let Some(query) = &args.query {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            config.device_query = None;
        } else {
            config.device_query = Some(trimmed.to_string());
        }
    }

    if let Some(query) = &config.device_query {
        eprintln!("device query: {query}");
    } else {
        eprintln!("device query: <none>");
    }

    let mut devices = discover_devices(&config)?;
    if devices.is_empty() {
        eprintln!("no devices found");
        return Ok(0);
    }

    devices.sort_by(|a, b| a.name.cmp(&b.name));
    for dev in devices {
        println!("{}", render_device(&dev));
    }
    Ok(0)
}

fn build_firmware_request(
    firmware: &FirmwareBuilder,
    args: &FirmwareBuildArgs,
) -> Result<BuildRequest, CliError> {
    let mut builder = firmware.builder().keyboard(args.keyboard.clone());
    if let Some(toolchain) = &args.toolchain {
        builder = builder.toolchain(toolchain.clone());
    }
    for target in &args.targets {
        builder = builder.target(target.clone());
    }
    builder = builder
        .output_dir(args.output_dir.clone())
        .disable_cache(args.disable_cache)
        .progress(Arc::new(CliProgressReporter));
    for pair in &args.env {
        let (key, value) = parse_env_var(pair)?;
        builder = builder.env(key, value);
    }
    for def in &args.kconfig_defs {
        let (key, value) = parse_kconfig_def(def)?;
        builder = builder.kconfig_def(key, value);
    }
    let builder = apply_firmware_layout(builder, args)?;
    Ok(builder.build()?)
}

fn apply_firmware_layout(
    mut builder: BuildRequestBuilder,
    args: &FirmwareBuildArgs,
) -> Result<BuildRequestBuilder, CliError> {
    use crate::cli::app::KeymapFormat;

    let path = args.layout.as_ref().ok_or_else(|| {
        CliError::FirmwareLayout("--layout is required".into())
    })?;

    let format = detect_layout_format(path, args.format.as_ref())?;

    // Warn if .json without explicit format (could be MoErgo)
    if path.extension().and_then(|e| e.to_str()) == Some("json") && args.format.is_none() {
        eprintln!(
            "warning: assuming standard JSON format; use --format moergo-json if this is a MoErgo layout"
        );
    }

    // Apply layout based on detected format
    match format {
        KeymapFormat::Json => builder = builder.layout_json_path(path.clone()),
        KeymapFormat::MoergoJson => {
            let text = io::read_text(path)?;
            let keymap = moergo::import_moergo_json(&text).map_err(CliError::Adapter)?;
            let layout: AdapterLayout = keymap.into();
            let standard_json = layout.to_standard_json().map_err(|err| {
                CliError::InvalidArgument(format!(
                    "failed to convert MoErgo layout to standard JSON: {err}"
                ))
            })?;
            let json_value = serde_json::from_str(&standard_json).map_err(|err| {
                CliError::InvalidArgument(format!(
                    "failed to parse converted MoErgo layout JSON: {err}"
                ))
            })?;
            builder = builder.layout_json_value(json_value);
        }
        KeymapFormat::Dts | KeymapFormat::Dtsi => {
            #[cfg(feature = "ancpp-preprocessor")]
            let layout = if args.preprocess.preprocess {
                let cfg = build_config(&args.preprocess, path)?;
                io::load_layout_preprocessed(path, &cfg)?
            } else {
                io::load_layout(path)?
            };
            #[cfg(not(feature = "ancpp-preprocessor"))]
            let layout = io::load_layout(path)?;

            let dts_text = layout
                .preprocessed_text
                .clone()
                .or(layout.raw_text.clone())
                .unwrap_or_default();
            let pipeline = AdapterPipeline::from_dts_text(dts_text);
            builder = builder.layout_via_pipeline(pipeline);
        }
    }

    Ok(builder)
}

/// Detect layout format from file extension, with optional explicit override.
fn detect_layout_format(
    path: &std::path::Path,
    explicit_format: Option<&crate::cli::app::KeymapFormat>,
) -> Result<crate::cli::app::KeymapFormat, CliError> {
    use crate::cli::app::KeymapFormat;

    if let Some(format) = explicit_format {
        return Ok(*format);
    }

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    match extension.as_deref() {
        Some("json") => Ok(KeymapFormat::Json),
        Some("dts") | Some("keymap") => Ok(KeymapFormat::Dts),
        Some("dtsi") => Ok(KeymapFormat::Dtsi),
        Some(ext) => Err(CliError::FirmwareLayout(format!(
            "unknown file extension '{}'; specify --format explicitly",
            ext
        ))),
        None => Err(CliError::FirmwareLayout(
            "--format is required when layout has no file extension".into(),
        )),
    }
}

fn parse_env_var(input: &str) -> Result<(String, String), CliError> {
    let mut parts = input.splitn(2, '=');
    let key = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::InvalidEnv(input.to_string()))?;
    let value = parts
        .next()
        .ok_or_else(|| CliError::InvalidEnv(input.to_string()))?;
    Ok((key.to_string(), value.to_string()))
}

fn parse_kconfig_def(input: &str) -> Result<(String, String), CliError> {
    let mut parts = input.splitn(2, '=');
    let key = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::InvalidKconfigDef(input.to_string()))?;
    let value = parts
        .next()
        .ok_or_else(|| CliError::InvalidKconfigDef(input.to_string()))?;
    Ok((key.to_string(), value.to_string()))
}

fn print_firmware_request(request: &BuildRequest) {
    let toolchain = match (&request.toolchain_id, request.keyboard_profile()) {
        (Some(id), _) => id.clone(),
        (None, Some(profile)) => format!("{} (default)", profile.default_toolchain),
        (None, None) => "<unknown>".into(),
    };
    if let Some(doc) = request.keyboard_profile_doc() {
        println!("keyboard : {} ({})", request.keyboard_id, doc.metadata.name);
        println!("vendor   : {}", doc.metadata.vendor);
        println!("firmware : {}", doc.firmware.default);
    } else {
        println!("keyboard : {}", request.keyboard_id);
    }
    println!("toolchain: {toolchain}");
    println!("targets  : {}", format_targets(request));
    println!("layout   : {}", describe_layout(&request.layout));
    println!("output   : {}", request.output_dir.display());
    println!(
        "cache    : {}",
        if request.disable_cache {
            "disabled"
        } else {
            "enabled"
        }
    );
    if request.extra_env.is_empty() {
        println!("env      : (none)");
    } else {
        for (key, value) in &request.extra_env {
            println!("env      : {key}={value}");
        }
    }
    if request.kconfig_defs.is_empty() {
        println!("kconfig  : (none)");
    } else {
        for (key, value) in &request.kconfig_defs {
            println!("kconfig  : {key}={value}");
        }
    }
}

fn print_build_report(report: &BuildReport) {
    if report.metadata.entries.is_empty() {
        println!("metadata: (none)");
    } else {
        println!("metadata:");
        for (key, value) in &report.metadata.entries {
            println!("  - {key}={value}");
        }
    }
    if report.artifacts.files.is_empty() {
        println!("artifacts: (none)");
    } else {
        println!("artifacts:");
        for path in &report.artifacts.files {
            println!("  - {}", path.display());
        }
    }
    match &report.logs_path {
        Some(path) => println!("logs     : {}", path.display()),
        None => println!("logs     : (not captured)"),
    }
    if let Some(path) = &report.build_info_path {
        println!("build-info: {}", path.display());
    } else {
        println!("build-info: (not written)");
    }
}

fn format_targets(request: &BuildRequest) -> String {
    request
        .targets
        .iter()
        .map(|target| target.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn describe_layout(source: &LayoutSource) -> String {
    match source {
        LayoutSource::JsonPath(path) => format!("json:{}", path.display()),
        LayoutSource::JsonValue(_) => "json:value".into(),
        LayoutSource::Document(_) => "dts:document".into(),
        LayoutSource::Pipeline(_) => "pipeline".into(),
        LayoutSource::Files { keymap, extra } => match extra {
            Some(config) => {
                format!("files:{} + {}", keymap.display(), config.display())
            }
            None => format!("files:{}", keymap.display()),
        },
    }
}

fn load_manifest_flexible(path: &PathBuf) -> Result<FirmwareManifest, CliError> {
    // Try loading as a direct path first
    match FirmwareManifest::from_file(path) {
        Ok(manifest) => Ok(manifest),
        Err(_) => {
            // If path doesn't exist and looks like a name (no separators, no .toml extension),
            // try loading from embedded profiles
            let path_str = path.to_string_lossy();
            if !path_str.contains('/') && !path_str.contains('\\') && !path_str.ends_with(".toml") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    return FirmwareManifest::load(name).map_err(Into::into);
                }
            }
            // Fall back to original error
            FirmwareManifest::from_file(path).map_err(Into::into)
        }
    }
}
