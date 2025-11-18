use clap::{Args, Parser, Subcommand, ValueEnum};
use similar::{ChangeTag, TextDiff};
use std::{fs, path::PathBuf};
use thiserror::Error;
use zmk_layout_rs::{
    dts::DtsDocument,
    providers::KeymapDocument,
    tasks::{
        ConflictPolicy, ExecutionMode, TaskConfigError, TaskEngineOptions, TaskExecution, TaskFile,
        TaskOutcome, TaskStatus, apply_tasks_with_options,
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
    Ok(print_results(&exec.results))
}

fn run_diff(args: &DiffArgs) -> Result<i32, CliError> {
    let PreparedContext {
        file,
        document,
        base_text,
    } = prepare(&args.shared)?;
    let exec = execute(document, &file, ExecutionMode::Apply);
    let code = print_results(&exec.results);
    if code != 0 {
        return Ok(code);
    }
    let updated = serialize_document(exec.document)?;
    print_diff(&base_text, &updated, &args.shared.base_layout);
    Ok(0)
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
}
