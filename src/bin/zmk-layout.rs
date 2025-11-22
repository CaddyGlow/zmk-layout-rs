use clap::{Args, Parser, Subcommand, ValueEnum};
use similar::{ChangeTag, TextDiff};
use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use zmk_layout_rs::{
    adapters::{
        AdapterError, TemplateParseMode, bundle::LayoutBundle, export_standard_str,
        export_standard_str_with_template_mode, import_standard_str_with_template,
        moergo::export_standard_str_from_moergo_dtsi, render_standard_template,
        template_contains_placeholders,
    },
    build::{
        BuildError, BuildReport, BuildRequest, BuildRequestBuilder, BuildRequestError,
        CliDockerBackend, CliProgressReporter, FirmwareBuilder, FirmwareManifest, LayoutSource,
    },
    dts::DtsDocument,
    flash::{
        FlashConfig, FlashError, FlashSideSelection, build_flash_targets, default_sides,
        discover_devices, flash_target, resolve_flash_source,
    },
    profiles::KeyboardProfileDoc,
    providers::KeymapDocument,
    tasks::{
        ConflictPolicy, ExecutionMode, TaskAction, TaskConfigError, TaskEngineOptions,
        TaskExecution, TaskFile, TaskOutcome, TaskStatus, apply_tasks_with_options,
    },
};

fn main() {
    let _ =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("")).try_init();
    if let Err(err) = run_cli() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run_cli() -> Result<(), CliError> {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Apply(args) => run_apply(&args)?,
        Command::Validate(args) => run_validate(&args)?,
        Command::Diff(args) => run_diff(&args)?,
        Command::Script(args) => run_script(&args)?,
        Command::Firmware(cmd) => run_firmware(cmd)?,
        Command::Profiles(cmd) => run_profiles(cmd)?,
        Command::Layer(cmd) => run_layer(cmd)?,
        Command::Bundle(cmd) => run_bundle(cmd)?,
    };
    std::process::exit(code);
}

#[derive(Parser)]
#[command(name = "zmk-layout", version, about = "Layout customization CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Apply(ApplyArgs),
    Validate(ValidateArgs),
    Diff(DiffArgs),
    Script(ScriptArgs),
    #[command(subcommand)]
    Firmware(FirmwareCommand),
    #[command(subcommand)]
    Profiles(ProfilesCommand),
    #[command(subcommand)]
    Layer(LayerCommand),
    #[command(subcommand)]
    Bundle(BundleCommand),
}

#[derive(Args, Clone)]
struct SharedArgs {
    #[arg(long, value_name = "FILE", help = "Task file to execute (TOML)")]
    tasks: PathBuf,
    #[arg(
        long = "base-layout",
        value_name = "DTS",
        help = "Base layout file to transform"
    )]
    base_layout: PathBuf,
    #[arg(
        long = "base-template",
        value_name = "NAME",
        help = "Documented template name for warnings"
    )]
    base_template: Option<String>,
    #[arg(
        long = "base-version",
        value_name = "VERSION",
        help = "Documented template version for warnings"
    )]
    base_version: Option<String>,
    #[arg(
        long = "conflicts",
        value_enum,
        help = "Override the default conflict policy (prompt/override/skip/script)"
    )]
    conflicts: Option<ConflictFlag>,
    #[arg(
        long = "combo-conditions",
        help = "Print a summary of combo conditions after task execution"
    )]
    combo_conditions: bool,
}

#[derive(Args, Clone)]
struct ApplyArgs {
    #[command(flatten)]
    shared: SharedArgs,
    #[arg(long, value_name = "FILE", help = "Write updated layout to this file")]
    output: Option<PathBuf>,
}

#[derive(Args, Clone)]
struct ValidateArgs {
    #[command(flatten)]
    shared: SharedArgs,
}

#[derive(Args, Clone)]
struct DiffArgs {
    #[command(flatten)]
    shared: SharedArgs,
}

#[derive(Args, Clone)]
struct ScriptArgs {
    #[arg(long, value_name = "FILE", help = "Lua script file to execute")]
    script: PathBuf,
    #[arg(long = "layout", value_name = "DTS", help = "Layout file to transform")]
    layout: PathBuf,
    #[arg(long, value_name = "FILE", help = "Write updated layout to this file")]
    output: Option<PathBuf>,
    #[arg(long = "diff", help = "Show diff instead of writing output")]
    show_diff: bool,
}

#[derive(Subcommand)]
enum FirmwareCommand {
    Build(FirmwareBuildArgs),
    Flash(FirmwareFlashArgs),
    Devices(FirmwareDevicesArgs),
}

#[derive(Subcommand)]
enum ProfilesCommand {
    Check(ProfileCheckArgs),
}

#[derive(Subcommand)]
enum LayerCommand {
    Export(LayerExportArgs),
    Import(LayerImportArgs),
}

#[derive(Subcommand)]
enum BundleCommand {
    Import(BundleImportArgs),
    Export(BundleExportArgs),
    Render(BundleRenderArgs),
}

#[derive(Args, Clone)]
struct BundleImportArgs {
    #[arg(value_enum, long, default_value_t = BundleFormat::Moergo, help = "Source layout format")]
    format: BundleFormat,
    #[arg(long, value_name = "FILE", help = "Input layout file")]
    input: PathBuf,
    #[arg(long, value_name = "FILE", help = "Destination bundle JSON")]
    output: PathBuf,
}

#[derive(Args, Clone)]
struct BundleExportArgs {
    #[arg(value_enum, long, default_value_t = BundleFormat::Moergo, help = "Export layout format")]
    format: BundleFormat,
    #[arg(long, value_name = "FILE", help = "Layout bundle JSON")]
    bundle: PathBuf,
    #[arg(long, value_name = "FILE", help = "Destination file to write")]
    output: PathBuf,
}

#[derive(Args, Clone)]
struct BundleRenderArgs {
    #[arg(long, value_name = "FILE", help = "Layout bundle JSON")]
    bundle: PathBuf,
    #[arg(long, value_name = "TARGET", help = "Target id inside the bundle")]
    target: String,
    #[arg(long, value_name = "FILE", help = "Override template path")]
    template: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Write rendered DTS to this file; prints to stdout when omitted"
    )]
    output: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum BundleFormat {
    Moergo,
}

#[derive(Args, Clone)]
struct ProfileCheckArgs {
    #[arg(
        value_name = "FILE",
        help = "Keyboard profile TOML file",
        num_args = 0..
    )]
    paths: Vec<PathBuf>,
    #[arg(long, help = "Validate every profile under --profiles-dir")]
    all: bool,
    #[arg(
        long = "profiles-dir",
        value_name = "DIR",
        default_value = "profiles/keyboards",
        help = "Directory scanned when --all is provided"
    )]
    profiles_dir: PathBuf,
}

#[derive(Args, Clone)]
struct LayerExportArgs {
    #[arg(long, value_name = "FILE", help = "Input DTS/.dtsi file to parse")]
    dts: PathBuf,
    #[arg(long, value_name = "FILE", help = "Destination JSON file to write")]
    json: PathBuf,
    #[arg(
        long,
        value_enum,
        help = "Use vendor-specific regex extraction instead of a template"
    )]
    vendor: Option<VendorExtractionFlag>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Optional template to extract metadata placeholders"
    )]
    template: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = TemplateModeFlag::Strip,
        help = "How to parse the DTS when a template is provided"
    )]
    template_mode: TemplateModeFlag,
}

#[derive(Args, Clone)]
struct LayerImportArgs {
    #[arg(long, value_name = "FILE", help = "Standard JSON layout file")]
    json: PathBuf,
    #[arg(
        long,
        value_name = "FILE",
        help = "DTS template that provides macros, includes, etc."
    )]
    template: PathBuf,
    #[arg(long, value_name = "FILE", help = "Output DTS path to write")]
    output: PathBuf,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum VendorExtractionFlag {
    Moergo,
}

#[derive(Args, Clone)]
struct FirmwareBuildArgs {
    #[arg(long, value_name = "FILE", help = "Firmware manifest (TOML)")]
    manifest: PathBuf,
    #[arg(
        long,
        value_name = "KEYBOARD",
        help = "Keyboard id declared in the manifest"
    )]
    keyboard: String,
    #[arg(long, value_name = "TOOLCHAIN", help = "Override toolchain id")]
    toolchain: Option<String>,
    #[arg(
        long = "target",
        value_name = "ID",
        help = "Target id to build (repeatable)"
    )]
    targets: Vec<String>,
    #[arg(
        long = "layout-json",
        value_name = "FILE",
        help = "Layout JSON file to consume"
    )]
    layout_json: Option<PathBuf>,
    #[arg(
        long = "layout-dts",
        value_name = "FILE",
        help = "DTS layout to use as input"
    )]
    layout_dts: Option<PathBuf>,
    #[arg(
        long = "keymap",
        value_name = "FILE",
        help = "Keymap Devicetree source (.keymap/.dtsi)"
    )]
    keymap: Option<PathBuf>,
    #[arg(
        long = "kconfig",
        value_name = "FILE",
        help = "Optional CONFIG overlay (.conf/.config.dtsi)"
    )]
    kconfig: Option<PathBuf>,
    #[arg(
        long = "output-dir",
        value_name = "DIR",
        help = "Directory where artifacts should land"
    )]
    output_dir: PathBuf,
    #[arg(
        long = "env",
        value_name = "KEY=VALUE",
        help = "Extra env vars forwarded to the toolchain"
    )]
    env: Vec<String>,
    #[arg(long, help = "Disable workspace/build cache hydration")]
    disable_cache: bool,
    #[arg(
        long,
        help = "Only print the resolved firmware request without running Docker"
    )]
    dry_run: bool,
}

#[derive(Args, Clone)]
struct FirmwareFlashArgs {
    #[arg(long, value_name = "FILE", help = "Firmware manifest (TOML)")]
    manifest: PathBuf,
    #[arg(
        long,
        value_name = "KEYBOARD",
        help = "Keyboard id declared in the manifest"
    )]
    keyboard: String,
    #[arg(
        long,
        value_enum,
        value_name = "SIDE",
        help = "Side to flash (default inferred from keyboard profile)"
    )]
    side: Option<FlashSideFlag>,
    #[arg(long, value_name = "FILE", help = "UF2 file to flash on every side")]
    firmware: Option<PathBuf>,
    #[arg(long, value_name = "FILE", help = "UF2 file to flash on the left half")]
    left: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "UF2 file to flash on the right half"
    )]
    right: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Build info JSON produced by `zmk-layout firmware build`"
    )]
    build_info: Option<PathBuf>,
    #[arg(
        long,
        value_name = "DIR",
        help = "Directory containing UF2 artifacts (auto-picks left/right hints)"
    )]
    artifacts: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Mounted bootloader volume to write the UF2 into"
    )]
    device: Option<PathBuf>,
    #[arg(
        long,
        value_name = "SECONDS",
        help = "Override the hardware.flash mount_timeout (seconds)"
    )]
    mount_timeout: Option<u64>,
    #[arg(
        long,
        value_name = "SECONDS",
        help = "Override the hardware.flash copy_timeout (seconds)"
    )]
    copy_timeout: Option<u64>,
    #[arg(long, help = "Skip sync after copy, even if requested by the profile")]
    no_sync: bool,
}

#[derive(Args, Clone)]
struct FirmwareDevicesArgs {
    #[arg(long, value_name = "FILE", help = "Firmware manifest (TOML)")]
    manifest: PathBuf,
    #[arg(
        long,
        value_name = "KEYBOARD",
        help = "Keyboard id declared in the manifest"
    )]
    keyboard: String,
    #[arg(
        long,
        value_name = "QUERY",
        help = "Override the hardware.flash device_query (default uses profile)"
    )]
    query: Option<String>,
    #[arg(long, help = "Ignore the profile query and list every detected device")]
    all: bool,
}

#[derive(Copy, Clone, ValueEnum)]
enum ConflictFlag {
    Prompt,
    Override,
    Skip,
    Script,
}

impl From<ConflictFlag> for ConflictPolicy {
    fn from(value: ConflictFlag) -> Self {
        match value {
            ConflictFlag::Prompt => ConflictPolicy::Prompt,
            ConflictFlag::Override => ConflictPolicy::Override,
            ConflictFlag::Skip => ConflictPolicy::Skip,
            ConflictFlag::Script => ConflictPolicy::Script,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum FlashSideFlag {
    Left,
    Right,
    Both,
}

impl From<FlashSideFlag> for FlashSideSelection {
    fn from(flag: FlashSideFlag) -> Self {
        match flag {
            FlashSideFlag::Left => FlashSideSelection::Left,
            FlashSideFlag::Right => FlashSideSelection::Right,
            FlashSideFlag::Both => FlashSideSelection::Both,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum TemplateModeFlag {
    Strip,
    Full,
}

impl From<TemplateModeFlag> for TemplateParseMode {
    fn from(flag: TemplateModeFlag) -> Self {
        match flag {
            TemplateModeFlag::Strip => TemplateParseMode::StripPlaceholders,
            TemplateModeFlag::Full => TemplateParseMode::FullDocument,
        }
    }
}

fn run_apply(args: &ApplyArgs) -> Result<i32, CliError> {
    let PreparedContext { file, document, .. } = prepare(&args.shared)?;
    let exec = execute(document, &file, ExecutionMode::Apply);
    let code = print_results(&exec.results);
    if args.shared.combo_conditions {
        print_combo_conditions(&file);
    }
    if code != 0 {
        return Ok(code);
    }

    let output = serialize_document(exec.document)?;
    if let Some(path) = &args.output {
        fs::write(path, output).map_err(|source| CliError::WriteFile {
            path: path.clone(),
            source,
        })?;
        eprintln!("wrote updated layout to {}", path.display());
    } else {
        print!("{}", output);
    }

    Ok(code)
}

fn run_validate(args: &ValidateArgs) -> Result<i32, CliError> {
    let PreparedContext { file, document, .. } = prepare(&args.shared)?;
    let exec = execute(document, &file, ExecutionMode::DryRun);
    let code = print_results(&exec.results);
    if args.shared.combo_conditions {
        print_combo_conditions(&file);
    }
    Ok(code)
}

fn run_diff(args: &DiffArgs) -> Result<i32, CliError> {
    let PreparedContext {
        file,
        document,
        base_text,
    } = prepare(&args.shared)?;
    let exec = execute(document, &file, ExecutionMode::Apply);
    let code = print_results(&exec.results);
    if args.shared.combo_conditions {
        print_combo_conditions(&file);
    }
    if code != 0 {
        return Ok(code);
    }
    let updated = serialize_document(exec.document)?;
    print_diff(&base_text, &updated, &args.shared.base_layout);
    Ok(0)
}

fn run_script(args: &ScriptArgs) -> Result<i32, CliError> {
    let script_text = fs::read_to_string(&args.script).map_err(|source| CliError::ReadFile {
        path: args.script.clone(),
        source,
    })?;

    let base_text = fs::read_to_string(&args.layout).map_err(|source| CliError::ReadFile {
        path: args.layout.clone(),
        source,
    })?;
    let dts = DtsDocument::parse_str(&base_text).map_err(|source| CliError::ParseLayout {
        path: args.layout.clone(),
        source,
    })?;
    let document = KeymapDocument::from_document(dts);

    let script_dir = args.script.parent().map(|p| {
        if p.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            p.to_path_buf()
        }
    });

    let result =
        zmk_layout_rs::tasks::execute_script(document, &script_text, script_dir.as_deref())?;

    if !result.logs.is_empty() {
        for log in &result.logs {
            eprintln!("{}", log);
        }
    }

    if let Some(error) = result.error {
        eprintln!("Script execution failed: {}", error);
        return Ok(2);
    }

    let output = serialize_document(result.document)?;

    if args.show_diff {
        print_diff(&base_text, &output, &args.layout);
    } else if let Some(path) = &args.output {
        fs::write(path, output).map_err(|source| CliError::WriteFile {
            path: path.clone(),
            source,
        })?;
        eprintln!("wrote updated layout to {}", path.display());
    } else {
        print!("{}", output);
    }

    Ok(0)
}

fn run_firmware(command: FirmwareCommand) -> Result<i32, CliError> {
    match command {
        FirmwareCommand::Build(args) => run_firmware_build(&args),
        FirmwareCommand::Flash(args) => run_firmware_flash(&args),
        FirmwareCommand::Devices(args) => run_firmware_devices(&args),
    }
}

fn run_profiles(command: ProfilesCommand) -> Result<i32, CliError> {
    match command {
        ProfilesCommand::Check(args) => run_profile_check(&args),
    }
}

fn run_layer(command: LayerCommand) -> Result<i32, CliError> {
    match command {
        LayerCommand::Export(args) => run_layer_export(&args),
        LayerCommand::Import(args) => run_layer_import(&args),
    }
}

fn run_bundle(command: BundleCommand) -> Result<i32, CliError> {
    match command {
        BundleCommand::Import(args) => run_bundle_import(&args),
        BundleCommand::Export(args) => run_bundle_export(&args),
        BundleCommand::Render(args) => run_bundle_render(&args),
    }
}

fn run_layer_export(args: &LayerExportArgs) -> Result<i32, CliError> {
    let source = fs::read_to_string(&args.dts).map_err(|source| CliError::ReadFile {
        path: args.dts.clone(),
        source,
    })?;

    if let Some(vendor) = args.vendor {
        if args.template.is_some() {
            return Err(CliError::InvalidArgument(
                "choose either --vendor or --template, not both".into(),
            ));
        }
        let contents = match vendor {
            VendorExtractionFlag::Moergo => export_standard_str_from_moergo_dtsi(&source)?,
        };
        fs::write(&args.json, contents).map_err(|source| CliError::WriteFile {
            path: args.json.clone(),
            source,
        })?;
    } else if let Some(template_path) = &args.template {
        let template_source =
            fs::read_to_string(template_path).map_err(|source| CliError::ReadFile {
                path: template_path.clone(),
                source,
            })?;
        let contents = export_standard_str_with_template_mode(
            &source,
            &template_source,
            args.template_mode.into(),
        )?;
        fs::write(&args.json, contents).map_err(|source| CliError::WriteFile {
            path: args.json.clone(),
            source,
        })?;
    } else {
        let document = DtsDocument::parse_str(&source).map_err(|source| CliError::ParseLayout {
            path: args.dts.clone(),
            source,
        })?;
        let contents = export_standard_str(&document)?;
        fs::write(&args.json, contents).map_err(|source| CliError::WriteFile {
            path: args.json.clone(),
            source,
        })?;
    }

    eprintln!("exported layout to {}", args.json.display());
    Ok(0)
}

fn run_layer_import(args: &LayerImportArgs) -> Result<i32, CliError> {
    let json_text = fs::read_to_string(&args.json).map_err(|source| CliError::ReadFile {
        path: args.json.clone(),
        source,
    })?;
    let template_source =
        fs::read_to_string(&args.template).map_err(|source| CliError::ReadFile {
            path: args.template.clone(),
            source,
        })?;

    if template_contains_placeholders(&template_source) {
        let rendered = render_standard_template(&json_text, &template_source)?;
        fs::write(&args.output, rendered).map_err(|source| CliError::WriteFile {
            path: args.output.clone(),
            source,
        })?;
    } else {
        let imported = import_standard_str_with_template(&json_text, &template_source)?;
        let rendered = imported.to_string().map_err(CliError::Serialize)?;
        fs::write(&args.output, rendered).map_err(|source| CliError::WriteFile {
            path: args.output.clone(),
            source,
        })?;
    }

    eprintln!("imported layout to {}", args.output.display());
    Ok(0)
}

fn run_bundle_import(args: &BundleImportArgs) -> Result<i32, CliError> {
    match args.format {
        BundleFormat::Moergo => {
            let bundle = LayoutBundle::from_moergo_file(&args.input)?;
            bundle.write_json(&args.output)?;
        }
    }
    eprintln!("imported bundle to {}", args.output.display());
    Ok(0)
}

fn run_bundle_export(args: &BundleExportArgs) -> Result<i32, CliError> {
    let bundle = LayoutBundle::from_json_file(&args.bundle)?;
    match args.format {
        BundleFormat::Moergo => {
            let json = bundle.to_moergo_json()?;
            fs::write(&args.output, json).map_err(|source| CliError::WriteFile {
                path: args.output.clone(),
                source,
            })?;
        }
    }
    eprintln!("exported bundle to {}", args.output.display());
    Ok(0)
}

fn run_bundle_render(args: &BundleRenderArgs) -> Result<i32, CliError> {
    let bundle = LayoutBundle::from_json_file(&args.bundle)?;
    let rendered = bundle.render_target(&args.target, args.template.as_deref())?;
    if let Some(path) = &args.output {
        fs::write(path, &rendered).map_err(|source| CliError::WriteFile {
            path: path.clone(),
            source,
        })?;
        eprintln!("rendered target {} to {}", args.target, path.display());
    } else {
        println!("{rendered}");
    }
    Ok(0)
}

fn run_firmware_flash(args: &FirmwareFlashArgs) -> Result<i32, CliError> {
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
        eprintln!(
            "flashed {} using {} ({} bytes)",
            outcome.side,
            outcome.mountpoint.display(),
            outcome.bytes_written
        );
        for warning in outcome.warnings {
            eprintln!("note: {warning}");
        }
    }
    Ok(0)
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

fn run_firmware_devices(args: &FirmwareDevicesArgs) -> Result<i32, CliError> {
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
        let mountpoints = if dev.mountpoints.is_empty() {
            "<not mounted>".to_string()
        } else {
            dev.mountpoints
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let dev_path = dev
            .dev_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".into());
        let fs_type = dev.fs_type.as_deref().unwrap_or("-");
        let serial = dev.serial.as_deref().unwrap_or("-");
        let vendor = dev.vendor.as_deref().unwrap_or("-");
        let model = dev.model.as_deref().unwrap_or("-");
        let removable = dev
            .removable
            .map(|r| if r { "removable" } else { "fixed" })
            .unwrap_or("-");
        println!(
            "{}  dev={}  mount={}  fs={}  serial={}  vendor={}  model={}  {}",
            dev.name, dev_path, mountpoints, fs_type, serial, vendor, model, removable
        );
    }
    Ok(0)
}

fn run_profile_check(args: &ProfileCheckArgs) -> Result<i32, CliError> {
    let mut requested = args.paths.clone();
    let mut from_embedded = false;
    if args.all {
        let discovered = discover_profile_paths(&args.profiles_dir)?;
        from_embedded = !args.profiles_dir.exists();
        requested.extend(discovered);
    }
    if requested.is_empty() {
        return Err(CliError::ProfileCheck(
            "provide at least one profile path or use --all".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut failures = 0;
    for path in requested {
        if !seen.insert(path.clone()) {
            continue;
        }

        // Try to load from file if it exists, otherwise try by name from embedded
        let result = if path.exists() {
            KeyboardProfileDoc::from_file(&path)
        } else if from_embedded {
            // Extract name from path for embedded loading
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                KeyboardProfileDoc::load(name)
            } else {
                KeyboardProfileDoc::from_file(&path)
            }
        } else {
            KeyboardProfileDoc::from_file(&path)
        };

        match result {
            Ok(profile) => {
                println!(
                    "[OK ] {} :: {} (keyboard `{}`)",
                    path.display(),
                    profile.metadata.name,
                    profile.keyboard
                );
            }
            Err(err) => {
                failures += 1;
                eprintln!("[ERR] {} :: {err}", path.display());
            }
        }
    }
    if failures == 0 { Ok(0) } else { Ok(2) }
}

fn discover_profile_paths(dir: &Path) -> Result<Vec<PathBuf>, CliError> {
    if dir.exists() {
        let read_dir = fs::read_dir(dir).map_err(|err| {
            CliError::ProfileCheck(format!("failed to read {}: {}", dir.display(), err))
        })?;
        let mut profiles = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|err| {
                CliError::ProfileCheck(format!("failed to enumerate {}: {}", dir.display(), err))
            })?;
            let path = entry.path();
            if matches!(path.extension().and_then(|ext| ext.to_str()), Some(ext) if ext.eq_ignore_ascii_case("toml"))
            {
                profiles.push(path);
            }
        }
        if !profiles.is_empty() {
            profiles.sort();
            return Ok(profiles);
        }
    }

    // Fallback to embedded profiles
    let available = KeyboardProfileDoc::list_available();
    if available.is_empty() {
        return Err(CliError::ProfileCheck(format!(
            "no profiles found in {} or embedded in binary",
            dir.display()
        )));
    }

    Ok(available
        .into_iter()
        .map(|name| dir.join(format!("{}.toml", name)))
        .collect())
}

fn run_firmware_build(args: &FirmwareBuildArgs) -> Result<i32, CliError> {
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
    if report.success { Ok(0) } else { Ok(2) }
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
    let builder = apply_firmware_layout(builder, args)?;
    Ok(builder.build()?)
}

fn apply_firmware_layout(
    mut builder: BuildRequestBuilder,
    args: &FirmwareBuildArgs,
) -> Result<BuildRequestBuilder, CliError> {
    let mut layout_set = false;
    if let Some(path) = &args.layout_json {
        builder = builder.layout_json_path(path.clone());
        layout_set = true;
    }
    if let Some(path) = &args.layout_dts {
        if layout_set {
            return Err(CliError::FirmwareLayout(
                "multiple layout inputs were provided".into(),
            ));
        }
        let text = fs::read_to_string(path).map_err(|source| CliError::ReadFile {
            path: path.clone(),
            source,
        })?;
        let document = DtsDocument::parse_str(&text).map_err(|source| CliError::ParseLayout {
            path: path.clone(),
            source,
        })?;
        builder = builder.layout_document(document);
        layout_set = true;
    }
    match (&args.keymap, &args.kconfig) {
        (Some(keymap), extra) => {
            if layout_set {
                return Err(CliError::FirmwareLayout(
                    "multiple layout inputs were provided".into(),
                ));
            }
            builder = builder.layout_files(keymap.clone(), extra.clone());
            layout_set = true;
        }
        (None, Some(_)) => {
            return Err(CliError::FirmwareLayout(
                "--kconfig requires --keymap".into(),
            ));
        }
        (None, None) => {}
    }
    if !layout_set {
        return Err(CliError::FirmwareLayout(
            "provide one of --layout-json, --layout-dts, or --keymap/--kconfig".into(),
        ));
    }
    Ok(builder)
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
        LayoutSource::Files { keymap, extra } => match extra {
            Some(config) => {
                format!("files:{} + {}", keymap.display(), config.display())
            }
            None => format!("files:{}", keymap.display()),
        },
    }
}

struct PreparedContext {
    file: TaskFile,
    document: KeymapDocument,
    base_text: String,
}

fn prepare(args: &SharedArgs) -> Result<PreparedContext, CliError> {
    let task_text = fs::read_to_string(&args.tasks).map_err(|source| CliError::ReadFile {
        path: args.tasks.clone(),
        source,
    })?;
    let mut file = TaskFile::from_toml_str(&task_text)?;
    if let Some(parent) = args.tasks.parent().map(|p| {
        if p.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            p.to_path_buf()
        }
    }) {
        file.set_script_dir(parent);
    }
    if let Some(policy) = args.conflicts.map(ConflictPolicy::from) {
        apply_conflict_override(&mut file, policy);
    }
    warn_base_metadata(&file, args);

    let base_text = fs::read_to_string(&args.base_layout).map_err(|source| CliError::ReadFile {
        path: args.base_layout.clone(),
        source,
    })?;
    let dts = DtsDocument::parse_str(&base_text).map_err(|source| CliError::ParseLayout {
        path: args.base_layout.clone(),
        source,
    })?;
    let document = KeymapDocument::from_document(dts);

    Ok(PreparedContext {
        file,
        document,
        base_text,
    })
}

fn execute(document: KeymapDocument, file: &TaskFile, mode: ExecutionMode) -> TaskExecution {
    let options = TaskEngineOptions { mode };
    apply_tasks_with_options(document, file, options)
}

fn serialize_document(document: KeymapDocument) -> Result<String, CliError> {
    let dts = document.into_document();
    dts.to_string().map_err(CliError::Serialize)
}

fn print_results(results: &[TaskOutcome]) -> i32 {
    let mut exit_code = 0;
    for result in results {
        let status = match result.status {
            TaskStatus::Applied => "APPLIED",
            TaskStatus::Skipped => "SKIPPED",
            TaskStatus::Conflict => {
                exit_code = exit_code.max(2);
                "CONFLICT"
            }
            TaskStatus::Error => {
                exit_code = exit_code.max(2);
                "ERROR"
            }
        };
        let description = result
            .message
            .clone()
            .unwrap_or_else(|| "completed".to_string());
        let mut detail = description;
        if let (Some(before), Some(after)) = (&result.before, &result.after) {
            if before != after {
                detail.push_str(&format!(" | {} => {}", before, after));
            }
        }
        println!(
            "[{status:>8}] {id:<20} {target} :: {detail}",
            id = result.id,
            target = result.target,
        );
    }
    exit_code
}

fn print_combo_conditions(file: &TaskFile) {
    let combos: Vec<_> = file
        .tasks
        .iter()
        .filter_map(|task| match &task.action {
            TaskAction::Combo(combo) if !combo.conditions.is_empty() => Some((task, combo)),
            _ => None,
        })
        .collect();
    if combos.is_empty() {
        return;
    }
    println!("combo conditions:");
    for (task, combo) in combos {
        println!(
            "  - {} ({}) :: {}",
            combo.name,
            task.target,
            combo.conditions.join(", ")
        );
    }
}

fn apply_conflict_override(file: &mut TaskFile, policy: ConflictPolicy) {
    let previous = file.config.default_conflict;
    file.config.default_conflict = policy;
    for task in &mut file.tasks {
        if task.conflict == previous {
            task.conflict = policy;
        }
    }
}

fn warn_base_metadata(file: &TaskFile, args: &SharedArgs) {
    if let (Some(cli), Some(cfg)) = (args.base_template.as_deref(), file.base.template.as_deref()) {
        if cli != cfg {
            eprintln!(
                "warning: [base].template `{}` differs from --base-template `{}`",
                cfg, cli
            );
        }
    }
    if let (Some(cli), Some(cfg)) = (args.base_version.as_deref(), file.base.version.as_deref()) {
        if cli != cfg {
            eprintln!(
                "warning: [base].version `{}` differs from --base-version `{}`",
                cfg, cli
            );
        }
    }
}

fn print_diff(base: &str, updated: &str, base_path: &PathBuf) {
    println!("--- {}", base_path.display());
    println!("+++ updated");
    let diff = TextDiff::from_lines(base, updated);
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => '-',
            ChangeTag::Insert => '+',
            ChangeTag::Equal => ' ',
        };
        print!("{}{}", sign, change);
        if !change.value().ends_with('\n') {
            println!();
        }
    }
}

#[derive(Debug, Error)]
enum CliError {
    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse task file: {0}")]
    TaskConfig(#[from] TaskConfigError),
    #[error("failed to parse layout {path}: {source}")]
    ParseLayout {
        path: PathBuf,
        source: zmk_layout_rs::tokenizer::LayoutError,
    },
    #[error("failed to serialize layout: {0}")]
    Serialize(zmk_layout_rs::serialization::SerializeError),
    #[error("failed to parse firmware manifest: {0}")]
    Manifest(#[from] zmk_layout_rs::build::ManifestError),
    #[error("invalid firmware build request: {0}")]
    FirmwareRequest(#[from] BuildRequestError),
    #[error("firmware build failed: {0}")]
    FirmwareBuild(#[from] zmk_layout_rs::build::BuildError),
    #[error("invalid firmware layout arguments: {0}")]
    FirmwareLayout(String),
    #[error("invalid env specification `{0}`, expected KEY=VALUE")]
    InvalidEnv(String),
    #[error("script execution error: {0}")]
    ScriptExecution(#[from] zmk_layout_rs::tasks::ScriptExecutionError),
    #[error("profile check error: {0}")]
    ProfileCheck(String),
    #[error("keyboard `{0}` not found in manifest")]
    UnknownKeyboard(String),
    #[error("keyboard `{0}` has no metadata.profile reference in the manifest")]
    MissingKeyboardProfile(String),
    #[error("flashing failed: {0}")]
    Flash(#[from] FlashError),
    #[error("adapter error: {0}")]
    Adapter(#[from] AdapterError),
    #[error("bundle error: {0}")]
    Bundle(#[from] zmk_layout_rs::adapters::bundle::BundleError),
    #[error("invalid arguments: {0}")]
    InvalidArgument(String),
}
