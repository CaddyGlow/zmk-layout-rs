use clap::{Args, Parser, Subcommand, ValueEnum};
use similar::{ChangeTag, TextDiff};
use std::{fs, path::PathBuf, sync::Arc};
use thiserror::Error;
use zmk_layout_rs::{
    build::{
        BuildReport, BuildRequest, BuildRequestBuilder, BuildRequestError, CliDockerBackend,
        CliProgressReporter, FirmwareBuilder, FirmwareManifest, LayoutSource,
    },
    dts::DtsDocument,
    providers::KeymapDocument,
    tasks::{
        ConflictPolicy, ExecutionMode, TaskAction, TaskConfigError, TaskEngineOptions,
        TaskExecution, TaskFile, TaskOutcome, TaskStatus, apply_tasks_with_options,
    },
};

fn main() {
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
    #[arg(long, value_name = "FILE", help = "Rhai script file to execute")]
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
    }
}

fn run_firmware_build(args: &FirmwareBuildArgs) -> Result<i32, CliError> {
    let manifest = FirmwareManifest::from_file(&args.manifest)?;
    let builder = FirmwareBuilder::new(manifest, Box::new(CliDockerBackend::new()));
    let request = build_firmware_request(&builder, args)?;
    print_firmware_request(&request);
    if args.dry_run {
        return Ok(0);
    }
    let report = builder.build(request)?;
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
    println!("keyboard : {}", request.keyboard_id);
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
}
