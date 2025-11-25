//! Task execution engine for the layout customization workflow.

pub use crate::layout_engine::MetadataMap;

use std::{cell::RefCell, rc::Rc};

use crate::{
    keymap::KeymapDocument,
    layout_engine::{LayoutEngine, LayoutEngineError},
    tasks::{
        config::{
            BehaviorTask, ComboTask, ConflictPolicy, LayerOrderMovement, LayerOrderTask, LayerTask,
            MetaTask, OverrideTask, ScriptTask, Task, TaskAction, TaskFile,
        },
        lua_engine::{
            ScriptDecision, ScriptEnvironment, ScriptGlobals, create_layout_lua, set_script_globals,
        },
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

/// Apply tasks using default options (mutating the document).
pub fn apply_tasks(document: KeymapDocument, file: &TaskFile) -> TaskExecution {
    apply_tasks_with_options(document, file, TaskEngineOptions::default())
}

/// Apply tasks with explicit engine options.
pub fn apply_tasks_with_options(
    document: KeymapDocument,
    file: &TaskFile,
    options: TaskEngineOptions,
) -> TaskExecution {
    let mut engine = LayoutEngine::new(document);
    let mut results = Vec::new();
    let script_env = ScriptEnvironment::new(file);

    for task in &file.tasks {
        let outcome = match &task.action {
            TaskAction::Override(action) => {
                apply_override_task(&mut engine, task, action, options.mode, &script_env)
            }
            TaskAction::Layer(action) => {
                apply_layer_task(&mut engine, task, action, options.mode, &script_env)
            }
            TaskAction::Combo(action) => {
                apply_combo_task(&mut engine, task, action, options.mode, &script_env)
            }
            TaskAction::LayerOrder(action) => {
                apply_layer_order_task(&mut engine, task, action, options.mode, &script_env)
            }
            TaskAction::Behavior(action) => {
                apply_behavior_task(&mut engine, task, action, options.mode, &script_env)
            }
            TaskAction::Meta(action) => {
                apply_meta_task(&mut engine, task, action, options.mode, &script_env)
            }
            TaskAction::Script(action) => {
                apply_script_task(&mut engine, task, action, options.mode, &script_env)
            }
        };
        results.push(outcome);
    }

    TaskExecution {
        document: engine.into_document(),
        results,
    }
}

fn apply_override_task(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &OverrideTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
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

    if !guard_expected_state(task, &mut outcome, scripts) {
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

fn apply_layer_task(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &LayerTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, engine.layer_to_string(&action.name));

    if !guard_expected_state(task, &mut outcome, scripts) {
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

fn apply_combo_task(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &ComboTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, engine.combo_to_string(&action.name));

    if !guard_expected_state(task, &mut outcome, scripts) {
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

fn apply_layer_order_task(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &LayerOrderTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, Some(engine.layer_order_to_string()));

    if !guard_expected_state(task, &mut outcome, scripts) {
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

fn apply_behavior_task(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &BehaviorTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, engine.behavior_to_string(&action.behavior));

    if !guard_expected_state(task, &mut outcome, scripts) {
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

fn apply_meta_task(
    engine: &mut LayoutEngine,
    task: &Task,
    action: &MetaTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = outcome_with_before(task, engine.meta_to_string(&action.key));

    if !guard_expected_state(task, &mut outcome, scripts) {
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

fn resolve_conflict(
    task: &Task,
    reason: String,
    scripts: &ScriptEnvironment,
) -> ConflictResolution {
    match task.conflict {
        ConflictPolicy::Override => {
            ConflictResolution::Proceed(Some(format!("conflict overridden: {}", reason)))
        }
        ConflictPolicy::Skip => {
            ConflictResolution::Skip(format!("skipped due to conflict: {}", reason))
        }
        ConflictPolicy::Prompt => ConflictResolution::Abort(reason),
        ConflictPolicy::Script => match scripts.run_conflict_script(task, &reason) {
            Ok(decision) => match decision {
                ScriptDecision::Override(message) => ConflictResolution::Proceed(message),
                ScriptDecision::Skip(message) => ConflictResolution::Skip(message),
                ScriptDecision::Abort(message) => ConflictResolution::Abort(message),
            },
            Err(err) => ConflictResolution::Abort(err),
        },
    }
}

fn ensure_expected_state(
    task: &Task,
    actual: Option<&str>,
    outcome: &mut TaskOutcome,
    scripts: &ScriptEnvironment,
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
    match resolve_conflict(task, reason, scripts) {
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

fn apply_script_task(
    layout: &mut LayoutEngine,
    task: &Task,
    action: &ScriptTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    let source = match scripts.load_source(&action.source) {
        Ok(value) => value,
        Err(err) => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(err);
            return outcome;
        }
    };

    let working_engine = layout.clone();
    let shared_engine = Rc::new(RefCell::new(working_engine));
    let logs = Rc::new(RefCell::new(Vec::new()));

    let lua = match create_layout_lua(shared_engine.clone(), logs.clone()) {
        Ok(lua) => lua,
        Err(err) => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(format!("script error: {err}"));
            return outcome;
        }
    };

    if let Err(err) = set_script_globals(
        &lua,
        &action.args,
        ScriptGlobals {
            task_id: Some(task.id.as_str()),
            target: Some(task.target.as_str()),
            comment: task.comment.as_deref(),
        },
    ) {
        outcome.status = TaskStatus::Error;
        outcome.message = Some(format!("script error: {err}"));
        return outcome;
    }

    let eval_result = lua.load(&source).set_name(task.id.as_str()).exec();
    match eval_result {
        Ok(_) => {
            let log_messages = logs
                .borrow()
                .iter()
                .filter(|msg| !msg.is_empty())
                .cloned()
                .collect::<Vec<_>>();
            if !log_messages.is_empty() {
                append_message(&mut outcome.message, log_messages.join(" | "));
            }
            if mode == ExecutionMode::Apply {
                *layout = shared_engine.borrow().clone();
            } else {
                append_message(
                    &mut outcome.message,
                    "dry-run: script changes not applied to document",
                );
            }
            outcome.status = TaskStatus::Applied;
        }
        Err(err) => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(format!("script error: {err}"));
        }
    }
    outcome
}

fn outcome_with_before(task: &Task, before: Option<String>) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    outcome.before = before;
    outcome
}

fn guard_expected_state(
    task: &Task,
    outcome: &mut TaskOutcome,
    scripts: &ScriptEnvironment,
) -> bool {
    let before_snapshot = outcome.before.clone();
    ensure_expected_state(task, before_snapshot.as_deref(), outcome, scripts)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{DtItem, DtNode};
    use crate::layout_engine::LayerSelector;
    use crate::tasks::TaskConfigError;
    use crate::tasks::config::{BaseSection, ConfigSection, ScriptSource};
    use std::path::PathBuf;
    use toml::Value as TomlValue;

    #[test]
    fn parses_sample_configuration() {
        let doc = r#"
[base]
template = "glove80-community"
version = "1.2.0"

[config]
format_version = "1.0.0"
default_conflict = "prompt"

[[tasks]]
id = "swap-tab-q"
type = "override"
path = "layers.base.bindings[5]"
value = "&kp ESC"
target = "layers.base.bindings[5]"
comment = "Make ESC easier to reach"

[[tasks]]
type = "combo"
name = "my_custom_combo"
key_positions = [0, 1, 2]
binding = "&kp TAB"
target = "combos.my_custom_combo"

[[tasks]]
type = "layer-order"
layer = "nav"
position = 1
target = "layers.order.nav"

[[tasks]]
type = "script"
filename = "custom.lua"
target = "scripts.external.custom"
"#;

        let config = TaskFile::from_toml_str(doc).expect("parse config");
        assert_eq!(config.base.template.as_deref(), Some("glove80-community"));
        assert_eq!(config.tasks.len(), 4);

        match &config.tasks[0].action {
            TaskAction::Override(task) => {
                assert_eq!(task.path, "layers.base.bindings[5]");
                assert_eq!(task.value, "&kp ESC");
            }
            other => panic!("expected override, got {other:?}"),
        }

        // combo task should auto-generate an id
        assert_eq!(config.tasks[1].id, "combo-combos-my-custom-combo");
        match &config.tasks[1].action {
            TaskAction::Combo(task) => {
                assert_eq!(task.key_positions, vec![0, 1, 2]);
                assert_eq!(task.binding, "&kp TAB");
            }
            other => panic!("expected combo, got {other:?}"),
        }

        match &config.tasks[2].action {
            TaskAction::LayerOrder(task) => match task.movement {
                LayerOrderMovement::Position(idx) => assert_eq!(idx, 1),
                _ => panic!("expected absolute position movement"),
            },
            other => panic!("expected layer-order, got {other:?}"),
        }

        match &config.tasks[3].action {
            TaskAction::Script(task) => match &task.source {
                ScriptSource::File(path) => assert_eq!(path, "custom.lua"),
                _ => panic!("expected file script"),
            },
            other => panic!("expected script, got {other:?}"),
        }
    }

    #[test]
    fn rejects_duplicate_targets() {
        let doc = r#"
[config]
format_version = "1.0.0"

[[tasks]]
type = "override"
path = "layers.base.bindings[0]"
value = "&kp A"
target = "layers.base.bindings[0]"

[[tasks]]
type = "override"
path = "layers.base.bindings[1]"
value = "&kp B"
target = "layers.base.bindings[0]"
"#;

        let err = TaskFile::from_toml_str(doc).expect_err("duplicate targets");
        assert!(
            matches!(err, TaskConfigError::DuplicateTarget(target) if target == "layers.base.bindings[0]")
        );
    }

    #[test]
    fn rejects_overlapping_targets() {
        let doc = r#"
[config]
format_version = "1.0.0"

[[tasks]]
type = "layer"
name = "nav"
bindings = ["&kp A"]
target = "layers.nav"

[[tasks]]
type = "override"
path = "layers.nav.bindings[0]"
value = "&kp B"
target = "layers.nav.bindings[0]"
"#;

        let err = TaskFile::from_toml_str(doc).expect_err("overlapping targets");
        assert!(matches!(
            err,
            TaskConfigError::OverlappingTarget { new, existing }
                if new == "layers.nav.bindings[0]" && existing == "layers.nav"
        ));
    }

    #[test]
    fn rejects_mismatched_override_target() {
        let doc = r#"
[config]
format_version = "1.0.0"

[[tasks]]
type = "override"
path = "layers.base.bindings[0]"
value = "&kp ESC"
target = "combos.wrong"
"#;

        let err = TaskFile::from_toml_str(doc).expect_err("mismatched override target");
        assert!(matches!(
            err,
            TaskConfigError::InvalidField { field, .. } if field == "target"
        ));
    }

    #[test]
    fn requires_format_version() {
        let doc = r#"
[config]
default_conflict = "prompt"

[[tasks]]
type = "override"
path = "layers.base.bindings[0]"
value = "&kp A"
target = "layers.base.bindings[0]"
"#;

        let err = TaskFile::from_toml_str(doc).expect_err("missing format version");
        assert!(matches!(err, TaskConfigError::MissingFormatVersion));
    }

    #[test]
    fn applies_override_task() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "override".into(),
            target: "layers.base.bindings[1]".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::Override(OverrideTask {
                path: "layers.base.bindings[1]".into(),
                value: "&kp SPACE".into(),
                from: None,
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results.len(), 1);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let engine = LayoutEngine::new(exec.document.clone());
        let updated = engine.layer_to_string("base").expect("layer snapshot");
        assert!(updated.contains("&kp SPACE"));
    }

    #[test]
    fn creates_combo_task() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "combo-add".into(),
            target: "combos.combo_new".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::Combo(ComboTask {
                name: "combo_new".into(),
                key_positions: vec![2, 3],
                binding: "&kp ENTER".into(),
                timeout_ms: Some(40),
                layers: Vec::new(),
                conditions: vec![],
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let engine = LayoutEngine::new(exec.document.clone());
        assert!(engine.combo_to_string("combo_new").is_some());
    }

    #[test]
    fn combo_task_supports_layer_names() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "combo-add".into(),
            target: "combos.combo_new".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::Combo(ComboTask {
                name: "combo_new".into(),
                key_positions: vec![2, 3],
                binding: "&kp ENTER".into(),
                timeout_ms: Some(40),
                layers: vec![LayerSelector::Name("nav".into())],
                conditions: vec![],
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let engine = LayoutEngine::new(exec.document.clone());
        let snapshot = engine.combo_to_string("combo_new").unwrap();
        assert!(
            snapshot.contains("layers=<1>") || snapshot.contains("layers=< 1 >"),
            "snapshot: {}",
            snapshot
        );
    }

    #[test]
    fn combo_task_preserves_conditions() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "combo-conditions".into(),
            target: "combos.combo_cond".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::Combo(ComboTask {
                name: "combo_cond".into(),
                key_positions: vec![0, 1],
                binding: "&kp TAB".into(),
                timeout_ms: None,
                layers: Vec::new(),
                conditions: vec!["layer_state == base".into(), "mods.shift".into()],
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);

        let combo = exec
            .document
            .combos
            .iter()
            .find(|combo| combo.name == "combo_cond")
            .expect("combo node");
        assert_eq!(
            combo.conditions,
            vec!["layer_state == base".to_string(), "mods.shift".to_string()]
        );

        let engine = LayoutEngine::new(exec.document.clone());
        let snapshot = engine.combo_to_string("combo_cond").unwrap();
        assert!(
            snapshot.contains("conditions=layer_state == base && mods.shift"),
            "snapshot missing conditions: {}",
            snapshot
        );
    }

    #[test]
    fn behavior_task_updates_settings() {
        let mut settings = MetadataMap::new();
        settings.insert(
            "bindings".into(),
            TomlValue::Array(vec![TomlValue::String("&kp ENTER".into())]),
        );
        settings.insert("tapping-term-ms".into(), TomlValue::Integer(350));
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "behavior-update".into(),
            target: "behaviors.simple_tap".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::Behavior(BehaviorTask {
                behavior: "simple_tap".into(),
                settings,
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let engine = LayoutEngine::new(exec.document.clone());
        let snapshot = engine
            .behavior_to_string("simple_tap")
            .expect("behavior snapshot");
        assert!(
            snapshot.contains("bindings=< &kp ENTER >"),
            "snapshot: {}",
            snapshot
        );
        assert!(
            snapshot.contains("tapping-term-ms=< 350 >"),
            "snapshot: {}",
            snapshot
        );
    }

    #[test]
    fn meta_task_updates_entries() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "meta-update".into(),
            target: "meta.author".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: Some("\"Alice\"".into()),
            action: TaskAction::Meta(MetaTask {
                key: "author".into(),
                value: TomlValue::String("Bob".into()),
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let engine = LayoutEngine::new(exec.document.clone());
        let snapshot = engine.meta_to_string("author").expect("meta entry");
        assert_eq!(snapshot, "\"Bob\"");
    }

    #[test]
    fn moves_layer_order() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "layer-order".into(),
            target: "layers.order.nav".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::LayerOrder(LayerOrderTask {
                layer: "nav".into(),
                movement: LayerOrderMovement::Position(0),
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let engine = LayoutEngine::new(exec.document.clone());
        assert_eq!(engine.layer_order_to_string(), "nav,base");
    }

    #[test]
    fn layer_order_moves_to_end() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "layer-order".into(),
            target: "layers.order.base".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::LayerOrder(LayerOrderTask {
                layer: "base".into(),
                movement: LayerOrderMovement::After("nav".into()),
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let engine = LayoutEngine::new(exec.document.clone());
        assert_eq!(engine.layer_order_to_string(), "nav,base");
    }

    #[test]
    fn layer_task_applies_metadata() {
        let mut metadata = MetadataMap::new();
        metadata.insert("display_name".into(), TomlValue::String("Primary".into()));
        metadata.insert(
            "color".into(),
            TomlValue::Array(vec![
                TomlValue::Integer(1),
                TomlValue::Integer(2),
                TomlValue::Integer(3),
            ]),
        );

        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "layer-update".into(),
            target: "layers.base".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::Layer(LayerTask {
                name: "base".into(),
                bindings: vec!["&kp Q".into(), "&kp W".into()],
                metadata,
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);

        let layer = exec
            .document
            .layers
            .iter()
            .find(|layer| layer.name == "base")
            .expect("layer node");
        assert_eq!(
            layer.properties.get("display_name").map(String::as_str),
            Some("\"Primary\"")
        );
        assert_eq!(
            layer.properties.get("color").map(String::as_str),
            Some("< 1 2 3 >")
        );
    }

    #[test]
    fn expected_snapshot_detects_conflict() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "layer-order".into(),
            target: "layers.order.nav".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: Some("nav,base".into()),
            action: TaskAction::LayerOrder(LayerOrderTask {
                layer: "nav".into(),
                movement: LayerOrderMovement::Position(0),
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Conflict);
        assert!(
            exec.results[0]
                .message
                .as_deref()
                .unwrap()
                .contains("expected `nav,base`")
        );
    }

    #[test]
    fn script_task_executes_inline_code() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "script-inline".into(),
            target: "layers.base".into(),
            conflict: ConflictPolicy::Prompt,
            comment: Some("inline script".into()),
            expected: None,
            action: TaskAction::Script(ScriptTask {
                source: ScriptSource::Inline(
                    r#"
log("inline start");
set_binding("base", 0, "&kp ESC");
"#
                    .into(),
                ),
                args: MetadataMap::new(),
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let engine = LayoutEngine::new(exec.document.clone());
        let updated = engine.layer_to_string("base").expect("layer snapshot");
        assert!(updated.contains("&kp ESC"));
        assert!(
            exec.results[0]
                .message
                .as_deref()
                .unwrap()
                .contains("inline start")
        );
    }

    #[test]
    fn script_task_loads_file_relative_to_task_dir() {
        let mut file = default_task_file();
        file.script_dir = Some(fixtures_dir());
        file.tasks.push(Task {
            id: "script-file".into(),
            target: "layers.base".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::Script(ScriptTask {
                source: ScriptSource::File("script_task_file.lua".into()),
                args: MetadataMap::new(),
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let engine = LayoutEngine::new(exec.document.clone());
        let updated = engine.layer_to_string("base").expect("layer snapshot");
        assert!(updated.contains("&kp TAB"));
    }

    #[test]
    fn script_task_uses_extended_api() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "script-extended".into(),
            target: "layers.base".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::Script(ScriptTask {
                source: ScriptSource::Inline(
                    r#"
layout:layer("base")
  :bindings({"&kp ESC", "&kp W"})
  :meta("display_name", "\"Primary\"")
  :apply()

layout:move_layer("nav", 1)

layout:combo("combo_new")
  :keys({1, 2})
  :binding("&kp ENTER")
  :timeout(50)
  :on_layers({"nav"})
  :when("layer_state == nav")
  :apply()
"#
                    .into(),
                ),
                args: MetadataMap::new(),
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);

        let layer = exec
            .document
            .layers
            .iter()
            .find(|layer| layer.name == "base")
            .expect("layer");
        assert_eq!(
            layer.properties.get("display_name").map(String::as_str),
            Some("\"Primary\"")
        );

        let engine = LayoutEngine::new(exec.document.clone());
        assert_eq!(engine.layer_order_to_string(), "nav,base");

        let engine = LayoutEngine::new(exec.document.clone());
        let snapshot = engine.combo_to_string("combo_new").unwrap();
        assert!(
            snapshot.contains("timeout-ms=< 50 >") || snapshot.contains("timeout-ms=<50>"),
            "snapshot: {}",
            snapshot
        );
        assert!(
            snapshot.contains("conditions=layer_state == nav"),
            "snapshot: {}",
            snapshot
        );
    }

    #[test]
    fn conflict_script_overrides_decision() {
        let mut file = default_task_file();
        file.config.conflict_script = Some("conflict_override.lua".into());
        file.script_dir = Some(fixtures_dir());
        file.tasks.push(Task {
            id: "script-conflict".into(),
            target: "layers.base.bindings[0]".into(),
            conflict: ConflictPolicy::Script,
            comment: None,
            expected: Some("&kp ESC".into()),
            action: TaskAction::Override(OverrideTask {
                path: "layers.base.bindings[0]".into(),
                value: "&kp ESC".into(),
                from: None,
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        assert!(
            exec.results[0]
                .message
                .as_deref()
                .unwrap()
                .contains("override via script")
        );
    }

    #[test]
    fn script_task_enforces_operation_limits() {
        let mut file = default_task_file();
        file.tasks.push(Task {
            id: "script-spin".into(),
            target: "layers.base".into(),
            conflict: ConflictPolicy::Prompt,
            comment: None,
            expected: None,
            action: TaskAction::Script(ScriptTask {
                source: ScriptSource::Inline(
                    r#"
                        local counter = 0
                        while true do
                            counter = counter + 1
                        end
                    "#
                    .into(),
                ),
                args: MetadataMap::new(),
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Error);
        assert!(
            exec.results[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("instruction limit"),
            "unexpected error: {:?}",
            exec.results[0].message
        );
    }

    fn default_task_file() -> TaskFile {
        TaskFile {
            base: BaseSection::default(),
            config: ConfigSection {
                format_version: "1.0.0".into(),
                default_conflict: ConflictPolicy::Prompt,
                conflict_script: None,
                comment: None,
            },
            tasks: Vec::new(),
            script_dir: None,
        }
    }

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn sample_dts() -> &'static str {
        r#"
keymap {
    base {
        bindings = < &kp Q &kp W &kp E >;
    };
    nav {
        bindings = < &kp A &kp B &kp C >;
    };
};

combos {
    combo_esc {
        key-positions = < 0 1 >;
        bindings = < &kp ESC >;
    };
};

behaviors {
    simple_tap {
        compatible = "zmk,behavior-hold-tap";
        #binding-cells = <0>;
        bindings = < &kp A >;
        tapping-term-ms = <200>;
    };
};

meta {
    author = "Alice";
};
"#
    }

    fn find_layer_node<'a>(items: &'a [DtItem], name: &str) -> Option<&'a DtNode> {
        for item in items {
            match item {
                DtItem::Node(node) => {
                    if node.name == name {
                        return Some(node);
                    }
                    if let Some(found) = find_layer_node(&node.children, name) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn find_child_node<'a>(parent: &'a DtNode, name: &str) -> Option<&'a DtNode> {
        for item in &parent.children {
            if let DtItem::Node(node) = item {
                if node.name == name {
                    return Some(node);
                }
            }
        }
        None
    }
}
