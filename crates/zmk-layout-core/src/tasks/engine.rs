//! Task execution engine for the layout customization workflow.

pub use crate::layout_engine::MetadataMap;

use crate::{
    keymap::KeymapDocument,
    layout_engine::{LayoutEngine, LayoutEngineError},
    tasks::{
        config::{
            BehaviorTask, ComboTask, ConflictPolicy, LayerOrderMovement, LayerOrderTask, LayerTask,
            MetaTask, OverrideTask, ScriptTask, Task, TaskAction, TaskFile,
        },
        script_backend::{NoScriptBackend, ScriptBackend, ScriptDecision},
        targets::parse_override_path,
    },
};

/// Determines whether tasks mutate the layout or just report potential changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Apply,
    DryRun,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        ExecutionMode::Apply
    }
}

/// Options that affect how tasks are executed.
#[derive(Debug, Clone, Copy)]
pub struct TaskEngineOptions {
    pub mode: ExecutionMode,
}

impl Default for TaskEngineOptions {
    fn default() -> Self {
        Self {
            mode: ExecutionMode::Apply,
        }
    }
}

/// Result of running the task engine.
#[derive(Debug)]
pub struct TaskExecution {
    pub document: KeymapDocument,
    pub results: Vec<TaskOutcome>,
}

impl TaskExecution {
    pub fn into_document(self) -> KeymapDocument {
        self.document
    }
}

/// Outcome for a single task application.
#[derive(Debug, Clone)]
pub struct TaskOutcome {
    pub id: String,
    pub target: String,
    pub status: TaskStatus,
    pub message: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
}

impl TaskOutcome {
    fn new(task: &Task) -> Self {
        Self {
            id: task.id.clone(),
            target: task.target.clone(),
            status: TaskStatus::Applied,
            message: None,
            before: None,
            after: None,
        }
    }
}

/// Status reported for each task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Applied,
    Skipped,
    Conflict,
    Error,
}

/// Apply tasks using default options (mutating the document) without script support.
pub fn apply_tasks(document: KeymapDocument, file: &TaskFile) -> TaskExecution {
    apply_tasks_with_backend(document, file, TaskEngineOptions::default(), &NoScriptBackend)
}

/// Apply tasks with explicit engine options and a script backend.
pub fn apply_tasks_with_backend<B: ScriptBackend>(
    document: KeymapDocument,
    file: &TaskFile,
    options: TaskEngineOptions,
    backend: &B,
) -> TaskExecution {
    let mut engine = LayoutEngine::new(document);
    let mut results = Vec::new();

    for task in &file.tasks {
        let outcome = match &task.action {
            TaskAction::Override(action) => {
                apply_override_task(&mut engine, task, action, options.mode, backend)
            }
            TaskAction::Layer(action) => {
                apply_layer_task(&mut engine, task, action, options.mode, backend)
            }
            TaskAction::Combo(action) => {
                apply_combo_task(&mut engine, task, action, options.mode, backend)
            }
            TaskAction::LayerOrder(action) => {
                apply_layer_order_task(&mut engine, task, action, options.mode, backend)
            }
            TaskAction::Behavior(action) => {
                apply_behavior_task(&mut engine, task, action, options.mode, backend)
            }
            TaskAction::Meta(action) => {
                apply_meta_task(&mut engine, task, action, options.mode, backend)
            }
            TaskAction::Script(action) => {
                apply_script_task(&mut engine, task, action, options.mode, backend)
            }
        };
        results.push(outcome);
    }

    TaskExecution {
        document: engine.into_document(),
        results,
    }
}

fn apply_override_task<B: ScriptBackend>(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &OverrideTask,
    mode: ExecutionMode,
    backend: &B,
) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    let (layer, slot) = match parse_override_path(&action.path) {
        Ok(values) => values,
        Err(message) => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(message);
            return outcome;
        }
    };

    let bindings = match engine.layer_bindings(&layer) {
        Ok(value) => value,
        Err(err) => {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    };
    if slot >= bindings.len() {
        outcome.status = TaskStatus::Error;
        outcome.message = Some(format!(
            "binding index {} out of range for layer `{}` (len {})",
            slot,
            layer,
            bindings.len()
        ));
        return outcome;
    }

    outcome.before = Some(bindings[slot].clone());

    let normalized = match engine.normalize_binding(&action.value) {
        Ok(val) => val,
        Err(err) => {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    };

    if !guard_expected_state(task, &mut outcome, backend) {
        return outcome;
    }

    if !apply_or_dry_run(
        mode,
        &mut outcome,
        Some("dry-run: override not applied (reporting desired result)"),
        || engine.set_binding(&layer, slot, &normalized),
    ) {
        return outcome;
    }
    outcome.after = Some(normalized);
    outcome.status = TaskStatus::Applied;
    outcome
}

fn apply_layer_task<B: ScriptBackend>(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &LayerTask,
    mode: ExecutionMode,
    backend: &B,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, engine.layer_to_string(&action.name));

    if !guard_expected_state(task, &mut outcome, backend) {
        return outcome;
    }

    let normalized = match engine.normalize_binding_list(&action.bindings) {
        Ok(values) => values,
        Err(err) => {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    };

    let metadata = if action.metadata.is_empty() {
        None
    } else {
        Some(LayoutEngine::metadata_to_properties(&action.metadata))
    };
    let dry_run_message = {
        let mut notes = vec!["dry-run: layer bindings not applied (reporting desired result)"];
        if metadata.is_some() {
            notes.push("dry-run: layer metadata not applied (reporting desired result)");
        }
        notes.join(" | ")
    };

    if !apply_or_dry_run(mode, &mut outcome, Some(dry_run_message.as_str()), || {
        engine.set_layer_bindings(&action.name, &normalized)?;
        if let Some(meta) = metadata.as_ref() {
            engine.set_layer_metadata(&action.name, meta)?;
        }
        Ok(())
    }) {
        return outcome;
    }
    outcome.after = Some(format_bindings_raw(&normalized));
    outcome.status = TaskStatus::Applied;
    outcome
}

fn apply_combo_task<B: ScriptBackend>(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &ComboTask,
    mode: ExecutionMode,
    backend: &B,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, engine.combo_to_string(&action.name));

    if !guard_expected_state(task, &mut outcome, backend) {
        return outcome;
    }

    let normalized_binding = match engine.normalize_binding(&action.binding) {
        Ok(value) => value,
        Err(err) => {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    };

    let layers = match engine.resolve_layer_selectors(&action.layers) {
        Ok(values) => values,
        Err(err) => {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    };

    if !apply_or_dry_run(
        mode,
        &mut outcome,
        Some("dry-run: combo task recorded but not applied to document"),
        || {
            engine.upsert_combo(
                &action.name,
                &normalized_binding,
                &action.key_positions,
                action.timeout_ms,
                &layers,
                &action.conditions,
            )
        },
    ) {
        return outcome;
    }
    outcome.after = engine.combo_to_string(&action.name);
    if !action.conditions.is_empty() {
        append_message(
            &mut outcome.message,
            format!("combo conditions: {}", action.conditions.join(", ")),
        );
    }
    outcome.status = TaskStatus::Applied;
    outcome
}

fn apply_layer_order_task<B: ScriptBackend>(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &LayerOrderTask,
    mode: ExecutionMode,
    backend: &B,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, Some(engine.layer_order_to_string()));

    if !guard_expected_state(task, &mut outcome, backend) {
        return outcome;
    }

    let names = engine.layer_names();
    let current_index = match names.iter().position(|name| name == &action.layer) {
        Some(idx) => idx,
        None => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(format!("layer `{}` not found", action.layer));
            return outcome;
        }
    };

    let target_index = match &action.movement {
        LayerOrderMovement::Position(pos) => (*pos).min(names.len().saturating_sub(1)),
        LayerOrderMovement::Before(reference) => {
            match names.iter().position(|name| name == reference) {
                Some(idx) => idx,
                None => {
                    outcome.status = TaskStatus::Error;
                    outcome.message = Some(format!("reference layer `{}` not found", reference));
                    return outcome;
                }
            }
        }
        LayerOrderMovement::After(reference) => {
            match names.iter().position(|name| name == reference) {
                Some(idx) => (idx + 1).min(names.len()),
                None => {
                    outcome.status = TaskStatus::Error;
                    outcome.message = Some(format!("reference layer `{}` not found", reference));
                    return outcome;
                }
            }
        }
    };

    if target_index == current_index {
        append_message(
            &mut outcome.message,
            format!("layer `{}` already at requested position", action.layer),
        );
        outcome.after = outcome.before.clone();
        outcome.status = TaskStatus::Applied;
        return outcome;
    }

    if !apply_or_dry_run(
        mode,
        &mut outcome,
        Some("dry-run: layer ordering not applied (reporting current order)"),
        || engine.reorder_layer(&action.layer, target_index),
    ) {
        return outcome;
    }

    outcome.after = Some(engine.layer_order_to_string());
    outcome.status = TaskStatus::Applied;
    outcome
}

fn apply_behavior_task<B: ScriptBackend>(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &BehaviorTask,
    mode: ExecutionMode,
    backend: &B,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, engine.behavior_to_string(&action.behavior));

    if !guard_expected_state(task, &mut outcome, backend) {
        return outcome;
    }

    if action.settings.is_empty() {
        outcome.status = TaskStatus::Skipped;
        append_message(
            &mut outcome.message,
            "behavior task skipped: settings cannot be empty",
        );
        return outcome;
    }

    if !apply_or_dry_run(
        mode,
        &mut outcome,
        Some("dry-run: behavior settings not applied (reporting desired result)"),
        || engine.set_behavior_settings(&action.behavior, &action.settings),
    ) {
        return outcome;
    }
    outcome.after = engine.behavior_to_string(&action.behavior);
    outcome.status = TaskStatus::Applied;
    outcome
}

fn apply_meta_task<B: ScriptBackend>(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &MetaTask,
    mode: ExecutionMode,
    backend: &B,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, engine.meta_to_string(&action.key));

    if !guard_expected_state(task, &mut outcome, backend) {
        return outcome;
    }

    if !apply_or_dry_run(
        mode,
        &mut outcome,
        Some("dry-run: meta entry not applied (reporting desired result)"),
        || engine.set_meta_entry(&action.key, &action.value),
    ) {
        return outcome;
    }
    outcome.after = engine.meta_to_string(&action.key);
    outcome.status = TaskStatus::Applied;
    outcome
}

enum ConflictResolution {
    Proceed(Option<String>),
    Skip(String),
    Abort(String),
}

fn resolve_conflict<B: ScriptBackend>(
    task: &Task,
    reason: String,
    backend: &B,
) -> ConflictResolution {
    match task.conflict {
        ConflictPolicy::Override => {
            ConflictResolution::Proceed(Some(format!("conflict overridden: {}", reason)))
        }
        ConflictPolicy::Skip => {
            ConflictResolution::Skip(format!("skipped due to conflict: {}", reason))
        }
        ConflictPolicy::Prompt => ConflictResolution::Abort(reason),
        ConflictPolicy::Script => match backend.run_conflict_script(task, &reason) {
            Ok(decision) => match decision {
                ScriptDecision::Override(message) => ConflictResolution::Proceed(message),
                ScriptDecision::Skip(message) => ConflictResolution::Skip(message),
                ScriptDecision::Abort(message) => ConflictResolution::Abort(message),
            },
            Err(err) => ConflictResolution::Abort(err),
        },
    }
}

fn ensure_expected_state<B: ScriptBackend>(
    task: &Task,
    actual: Option<&str>,
    outcome: &mut TaskOutcome,
    backend: &B,
) -> bool {
    let Some(expected) = task.expected.as_deref() else {
        return true;
    };
    let actual_value = actual.map(|value| value.trim().to_string());
    if actual_value
        .as_deref()
        .map(|value| value == expected)
        .unwrap_or(false)
    {
        return true;
    }
    let actual_display = actual_value.as_deref().unwrap_or("<missing>");
    let reason = format!(
        "expected `{}` for target `{}` but found `{}`",
        expected, task.target, actual_display
    );
    match resolve_conflict(task, reason, backend) {
        ConflictResolution::Proceed(message) => {
            if let Some(msg) = message {
                append_message(&mut outcome.message, msg);
            }
            true
        }
        ConflictResolution::Skip(message) => {
            outcome.status = TaskStatus::Skipped;
            append_message(&mut outcome.message, message);
            false
        }
        ConflictResolution::Abort(message) => {
            outcome.status = TaskStatus::Conflict;
            append_message(&mut outcome.message, message);
            false
        }
    }
}

fn apply_script_task<B: ScriptBackend>(
    _layout: &mut LayoutEngine,
    task: &Task,
    action: &ScriptTask,
    _mode: ExecutionMode,
    backend: &B,
) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    let _source = match backend.load_source(&action.source) {
        Ok(value) => value,
        Err(err) => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(err);
            return outcome;
        }
    };

    // Script execution is delegated to the backend implementation
    // The NoScriptBackend will return an error for script tasks
    outcome.status = TaskStatus::Error;
    outcome.message = Some("script tasks require a script backend implementation".to_string());
    outcome
}

fn outcome_with_before(task: &Task, before: Option<String>) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    outcome.before = before;
    outcome
}

fn guard_expected_state<B: ScriptBackend>(
    task: &Task,
    outcome: &mut TaskOutcome,
    backend: &B,
) -> bool {
    let before_snapshot = outcome.before.clone();
    ensure_expected_state(task, before_snapshot.as_deref(), outcome, backend)
}

fn apply_or_dry_run(
    mode: ExecutionMode,
    outcome: &mut TaskOutcome,
    dry_run_message: Option<&str>,
    op: impl FnOnce() -> Result<(), LayoutEngineError>,
) -> bool {
    match mode {
        ExecutionMode::Apply => {
            if let Err(err) = op() {
                apply_engine_error(outcome, err);
                return false;
            }
        }
        ExecutionMode::DryRun => {
            if let Some(message) = dry_run_message {
                append_message(&mut outcome.message, message);
            }
        }
    }
    true
}

fn append_message(target: &mut Option<String>, note: impl Into<String>) {
    let note = note.into();
    match target {
        Some(existing) if !existing.is_empty() => {
            existing.push_str(" | ");
            existing.push_str(&note);
        }
        Some(existing) => {
            existing.push_str(&note);
        }
        None => {
            *target = Some(note);
        }
    }
}

fn apply_engine_error(outcome: &mut TaskOutcome, err: LayoutEngineError) {
    outcome.status = TaskStatus::Error;
    append_message(&mut outcome.message, err.to_string());
}

fn format_bindings_raw(bindings: &[String]) -> String {
    format_list(bindings)
}

fn format_list(values: &[String]) -> String {
    if values.is_empty() {
        "< >".to_string()
    } else {
        format!("< {} >", values.join(" "))
    }
}
