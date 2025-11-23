use std::path::PathBuf;

use crate::{
    io::{self, LoadedLayout},
    providers::KeymapDocument,
    tasks::{
        ConflictPolicy, ExecutionMode, TaskAction, TaskEngineOptions, TaskExecution, TaskFile,
        TaskOutcome, TaskStatus, apply_tasks_with_options,
    },
};

use super::{app::SharedArgs, error::CliError};

pub struct PreparedContext {
    pub file: TaskFile,
    pub layout: LoadedLayout,
    pub document: KeymapDocument,
}

pub fn prepare(args: &SharedArgs) -> Result<PreparedContext, CliError> {
    let loaded = io::load_task_file(&args.tasks)?;
    let mut file = loaded.file;
    if let Some(parent) = loaded.path.parent().map(|p| {
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

    let layout = io::load_layout(&args.base_layout)?;
    let document = KeymapDocument::from_document(layout.document.clone());
    Ok(PreparedContext {
        file,
        layout,
        document,
    })
}

pub fn execute(document: KeymapDocument, file: &TaskFile, mode: ExecutionMode) -> TaskExecution {
    let options = TaskEngineOptions { mode };
    apply_tasks_with_options(document, file, options)
}

pub fn print_results(results: &[TaskOutcome]) -> i32 {
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
            .as_deref()
            .unwrap_or_else(|| status_message(status));
        println!("[{}] {} :: {}", status, result.id, description);
    }
    exit_code
}

pub fn print_combo_conditions(file: &TaskFile) {
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

fn status_message(status: &str) -> &str {
    match status {
        "APPLIED" => "applied",
        "SKIPPED" => "skipped",
        "CONFLICT" => "conflict",
        "ERROR" => "error",
        _ => "unknown",
    }
}
