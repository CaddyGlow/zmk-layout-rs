//! Task configuration parser for the layout customization workflow.
//!
//! Phase 1 focuses on ingesting the TOML schema, validating structure,
//! auto-generating missing task identifiers, and enforcing per-task
//! targets so later phases can reason about conflicts.

use rhai::{
    Array as RhaiArray, Dynamic, Engine, EvalAltResult, FLOAT, INT, Map as RhaiMap, Position,
    Scope, module_resolvers::DummyModuleResolver,
};
use serde::Deserialize;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};
use thiserror::Error;
use toml::{Value as TomlValue, map::Map as TomlMap};

use crate::{
    ast::{DtItem, DtNode, DtProperty},
    bindings::BindingParser,
    dts::DtsDocument,
    providers::{COMBO_CONDITION_COMMENT_PREFIX, KeymapDocument, ProviderError},
};

/// Convenience alias for arbitrary TOML metadata blobs.
pub type MetadataMap = BTreeMap<String, TomlValue>;

/// Parsed representation of a task configuration file.
#[derive(Debug, Clone)]
pub struct TaskFile {
    pub base: BaseSection,
    pub config: ConfigSection,
    pub tasks: Vec<Task>,
    pub script_dir: Option<PathBuf>,
}

impl TaskFile {
    /// Parse a task file from raw TOML text.
    pub fn from_toml_str(contents: &str) -> Result<Self, TaskConfigError> {
        let raw: RawTaskFile = toml::from_str(contents).map_err(TaskConfigError::Parse)?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawTaskFile) -> Result<Self, TaskConfigError> {
        let base = raw.base.unwrap_or_default();
        let config = raw.config.ok_or(TaskConfigError::MissingConfig)?;
        let config: ConfigSection = config.try_into()?;

        if raw.tasks.is_empty() {
            return Err(TaskConfigError::NoTasks);
        }

        let mut ids = HashSet::new();
        let mut slug_counts = HashMap::new();
        let mut target_set = HashSet::new();
        let mut target_hierarchy = Vec::new();
        let mut tasks = Vec::with_capacity(raw.tasks.len());

        for (index, mut raw_task) in raw.tasks.into_iter().enumerate() {
            let kind = raw_task.kind;
            let target = match raw_task.target.as_ref() {
                Some(value) if !value.trim().is_empty() => normalize_locator(value),
                Some(_) => {
                    return Err(invalid(
                        kind,
                        "target",
                        index,
                        "target must be a non-empty string",
                    ));
                }
                None => return Err(missing(kind, "target", index)),
            };
            if !target_set.insert(target.clone()) {
                return Err(TaskConfigError::DuplicateTarget(target.clone()));
            }
            if let Some(existing) = find_overlapping_target(&target_hierarchy, &target) {
                return Err(TaskConfigError::OverlappingTarget {
                    existing,
                    new: target.clone(),
                });
            }
            target_hierarchy.push(target.clone());

            let id = match raw_task.id.clone() {
                Some(id) => {
                    if !ids.insert(id.clone()) {
                        return Err(TaskConfigError::DuplicateId(id));
                    }
                    id
                }
                None => {
                    let auto = auto_id(raw_task.kind, &target, &mut slug_counts);
                    if !ids.insert(auto.clone()) {
                        return Err(TaskConfigError::DuplicateId(auto));
                    }
                    auto
                }
            };

            let conflict = raw_task.conflict.unwrap_or(config.default_conflict);
            let comment = raw_task.comment.clone();

            let mut expected = raw_task.expected.take();
            if let Some(text) = expected.as_ref() {
                if text.trim().is_empty() {
                    return Err(invalid(
                        raw_task.kind,
                        "expected",
                        index,
                        "expected value cannot be empty",
                    ));
                }
            }

            let action = raw_task.into_action(index)?;
            if expected.is_none() {
                expected = match &action {
                    TaskAction::Override(action) => action.from.clone(),
                    _ => None,
                };
            }
            let expected = expected.map(|value| value.trim().to_string());

            if let TaskAction::Override(action) = &action {
                if normalize_locator(&action.path) != target {
                    return Err(invalid(
                        kind,
                        "target",
                        index,
                        format!(
                            "override target `{}` must match path `{}`",
                            target, action.path
                        ),
                    ));
                }
            }

            tasks.push(Task {
                id,
                target,
                conflict,
                comment,
                expected,
                action,
            });
        }

        Ok(TaskFile {
            base: base.into(),
            config,
            tasks,
            script_dir: None,
        })
    }

    /// Attach the directory used to resolve script paths.
    pub fn set_script_dir(&mut self, dir: impl Into<PathBuf>) {
        self.script_dir = Some(dir.into());
    }
}

#[derive(Debug, Clone)]
pub struct BaseSection {
    pub template: Option<String>,
    pub version: Option<String>,
    pub metadata: MetadataMap,
}

impl Default for BaseSection {
    fn default() -> Self {
        Self {
            template: None,
            version: None,
            metadata: MetadataMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigSection {
    pub format_version: String,
    pub default_conflict: ConflictPolicy,
    pub conflict_script: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub target: String,
    pub conflict: ConflictPolicy,
    pub comment: Option<String>,
    pub expected: Option<String>,
    pub action: TaskAction,
}

#[derive(Debug, Clone)]
pub enum TaskAction {
    Override(OverrideTask),
    Combo(ComboTask),
    Layer(LayerTask),
    LayerOrder(LayerOrderTask),
    Behavior(BehaviorTask),
    Meta(MetaTask),
    Script(ScriptTask),
}

#[derive(Debug, Clone)]
pub struct OverrideTask {
    pub path: String,
    pub value: String,
    pub from: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ComboTask {
    pub name: String,
    pub key_positions: Vec<u32>,
    pub binding: String,
    pub timeout_ms: Option<u32>,
    pub layers: Vec<LayerSelector>,
    pub conditions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LayerTask {
    pub name: String,
    pub bindings: Vec<String>,
    pub metadata: MetadataMap,
}

#[derive(Debug, Clone)]
pub enum LayerOrderMovement {
    Position(usize),
    Before(String),
    After(String),
}

#[derive(Debug, Clone)]
pub struct LayerOrderTask {
    pub layer: String,
    pub movement: LayerOrderMovement,
}

#[derive(Debug, Clone)]
pub struct BehaviorTask {
    pub behavior: String,
    pub settings: MetadataMap,
}

#[derive(Debug, Clone)]
pub struct MetaTask {
    pub key: String,
    pub value: TomlValue,
}

#[derive(Debug, Clone)]
pub enum LayerSelector {
    Index(u32),
    Name(String),
}

#[derive(Debug, Clone)]
pub enum ScriptSource {
    Inline(String),
    File(String),
}

#[derive(Debug, Clone)]
pub struct ScriptTask {
    pub source: ScriptSource,
    pub args: MetadataMap,
}

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
    mut document: KeymapDocument,
    file: &TaskFile,
    options: TaskEngineOptions,
) -> TaskExecution {
    let mut results = Vec::new();
    let mut parser = BindingParser::new();
    let script_env = ScriptEnvironment::new(file);

    for task in &file.tasks {
        let outcome = match &task.action {
            TaskAction::Override(action) => apply_override_task(
                &mut document,
                task,
                action,
                options.mode,
                &mut parser,
                &script_env,
            ),
            TaskAction::Layer(action) => apply_layer_task(
                &mut document,
                task,
                action,
                options.mode,
                &mut parser,
                &script_env,
            ),
            TaskAction::Combo(action) => apply_combo_task(
                &mut document,
                task,
                action,
                options.mode,
                &mut parser,
                &script_env,
            ),
            TaskAction::LayerOrder(action) => {
                apply_layer_order_task(&mut document, task, action, options.mode, &script_env)
            }
            TaskAction::Behavior(_) | TaskAction::Meta(_) => TaskOutcome {
                status: TaskStatus::Skipped,
                message: Some("behavior/meta tasks not implemented yet".into()),
                ..TaskOutcome::new(task)
            },
            TaskAction::Script(action) => {
                apply_script_task(&mut document, task, action, options.mode, &script_env)
            }
        };
        results.push(outcome);
    }

    TaskExecution { document, results }
}

fn apply_override_task(
    document: &mut KeymapDocument,
    task: &Task,
    action: &OverrideTask,
    mode: ExecutionMode,
    parser: &mut BindingParser,
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

    let normalized = match normalize_binding(parser, &action.value) {
        Ok(val) => val,
        Err(message) => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(message);
            return outcome;
        }
    };

    let bindings = match document.bindings_for_layer(&layer) {
        Ok(value) => value,
        Err(err) => {
            apply_provider_error(&mut outcome, err);
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

    let before_value = Some(bindings[slot].to_binding_string());
    outcome.before = before_value.clone();

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
        return outcome;
    }

    if mode == ExecutionMode::Apply {
        if let Err(err) = document.set_binding(&layer, slot, &normalized) {
            apply_provider_error(&mut outcome, err);
            return outcome;
        }
    } else {
        append_message(
            &mut outcome.message,
            "dry-run: override not applied (reporting desired result)",
        );
    }
    outcome.after = Some(normalized);
    outcome.status = TaskStatus::Applied;
    outcome
}

fn apply_layer_task(
    document: &mut KeymapDocument,
    task: &Task,
    action: &LayerTask,
    mode: ExecutionMode,
    parser: &mut BindingParser,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    let before = layer_snapshot(document.document(), &action.name);
    outcome.before = before.clone();

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
        return outcome;
    }

    let normalized = match normalize_binding_list(parser, &action.bindings) {
        Ok(values) => values,
        Err(message) => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(message);
            return outcome;
        }
    };

    if mode == ExecutionMode::Apply {
        if let Err(err) = document.set_layer_bindings(&action.name, &normalized) {
            apply_provider_error(&mut outcome, err);
            return outcome;
        }
        if !action.metadata.is_empty() {
            let metadata = metadata_properties(&action.metadata);
            if let Err(err) = document.set_layer_metadata(&action.name, &metadata) {
                apply_provider_error(&mut outcome, err);
                return outcome;
            }
        }
    } else {
        append_message(
            &mut outcome.message,
            "dry-run: layer bindings not applied (reporting desired result)",
        );
        if !action.metadata.is_empty() {
            append_message(
                &mut outcome.message,
                "dry-run: layer metadata not applied (reporting desired result)",
            );
        }
    }
    outcome.after = Some(format_bindings_raw(&normalized));
    outcome.status = TaskStatus::Applied;
    outcome
}

fn apply_combo_task(
    document: &mut KeymapDocument,
    task: &Task,
    action: &ComboTask,
    mode: ExecutionMode,
    parser: &mut BindingParser,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    outcome.before = combo_snapshot(document.document(), &action.name);

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
        return outcome;
    }

    let normalized_binding = match normalize_binding(parser, &action.binding) {
        Ok(value) => value,
        Err(message) => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(message);
            return outcome;
        }
    };

    let layers = match resolve_combo_layers(document, &action.layers) {
        Ok(values) => values,
        Err(message) => {
            outcome.status = TaskStatus::Error;
            outcome.message = Some(message);
            return outcome;
        }
    };

    if mode == ExecutionMode::Apply {
        if let Err(err) = document.upsert_combo(
            &action.name,
            &normalized_binding,
            &action.key_positions,
            action.timeout_ms,
            &layers,
            &action.conditions,
        ) {
            apply_provider_error(&mut outcome, err);
            return outcome;
        }
    } else {
        append_message(
            &mut outcome.message,
            "dry-run: combo task recorded but not applied to document",
        );
    }
    outcome.after = combo_snapshot(document.document(), &action.name);
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
    document: &mut KeymapDocument,
    task: &Task,
    action: &LayerOrderTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    let before = layer_order_snapshot(document.document());
    outcome.before = Some(before);

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
        return outcome;
    }

    let names = document.layer_names();
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

    if mode == ExecutionMode::Apply {
        if let Err(err) = document.reorder_layer(&action.layer, target_index) {
            apply_provider_error(&mut outcome, err);
            return outcome;
        }
    } else {
        append_message(
            &mut outcome.message,
            "dry-run: layer ordering not applied (reporting current order)",
        );
    }

    outcome.after = Some(layer_order_snapshot(document.document()));
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
        ConflictPolicy::Script => match scripts.resolve_conflict(task, &reason) {
            Ok(resolution) => resolution,
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

struct ScriptEnvironment<'a> {
    script_dir: Option<&'a Path>,
    conflict_script: Option<&'a str>,
}

const SCRIPT_MAX_OPERATIONS: u64 = 100_000;
const SCRIPT_MAX_CALL_DEPTH: usize = 64;

impl<'a> ScriptEnvironment<'a> {
    fn new(file: &'a TaskFile) -> Self {
        Self {
            script_dir: file.script_dir.as_deref(),
            conflict_script: file.config.conflict_script.as_deref(),
        }
    }

    fn resolve_conflict(&self, task: &Task, reason: &str) -> Result<ConflictResolution, String> {
        let script_path = self.conflict_script.ok_or_else(|| {
            "config.conflict_script must be set before using script conflicts".to_string()
        })?;
        let source = self
            .read_script(script_path)
            .map_err(|err| format!("failed to read conflict script `{}`: {}", script_path, err))?;
        let engine = sandboxed_engine();
        let ast = engine
            .compile(&source)
            .map_err(|err| format!("conflict script compile error: {err}"))?;
        let mut scope = Scope::new();
        let payload = build_conflict_payload(task, reason);
        let result: RhaiMap = engine
            .call_fn(&mut scope, &ast, "resolve", (payload,))
            .map_err(|err| format!("conflict script execution failed: {err}"))?;
        parse_conflict_resolution(result)
    }

    fn load_source(&self, source: &ScriptSource) -> Result<String, String> {
        match source {
            ScriptSource::Inline(code) => Ok(code.clone()),
            ScriptSource::File(path) => self
                .read_script(path)
                .map_err(|err| format!("failed to read script `{}`: {}", path, err)),
        }
    }

    fn read_script(&self, path: &str) -> Result<String, std::io::Error> {
        let resolved = resolve_script_path(self.script_dir, path);
        fs::read_to_string(&resolved)
    }
}

fn build_conflict_payload(task: &Task, reason: &str) -> RhaiMap {
    let mut payload = RhaiMap::new();
    payload.insert("id".into(), task.id.clone().into());
    payload.insert("target".into(), task.target.clone().into());
    payload.insert("reason".into(), reason.into());
    if let Some(comment) = &task.comment {
        payload.insert("comment".into(), comment.clone().into());
    }
    if let Some(expected) = &task.expected {
        payload.insert("expected".into(), expected.clone().into());
    }
    payload
}

fn parse_conflict_resolution(map: RhaiMap) -> Result<ConflictResolution, String> {
    let action = map
        .get("action")
        .and_then(|value| value.clone().into_string().ok())
        .ok_or_else(|| "conflict script must return a map with `action`".to_string())?;
    let message = map
        .get("message")
        .and_then(|value| value.clone().into_string().ok());
    match action.as_str() {
        "override" => Ok(ConflictResolution::Proceed(message)),
        "skip" => Ok(ConflictResolution::Skip(
            message.unwrap_or_else(|| "skipped by script".into()),
        )),
        "abort" => Ok(ConflictResolution::Abort(
            message.unwrap_or_else(|| "aborted by script".into()),
        )),
        other => Err(format!("unknown conflict action `{}`", other)),
    }
}

fn apply_script_task(
    document: &mut KeymapDocument,
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

    let working_doc = document.clone();
    let shared_doc = Rc::new(RefCell::new(working_doc));
    let logs = Rc::new(RefCell::new(Vec::new()));

    let mut engine = sandboxed_engine();
    register_script_api(&mut engine, shared_doc.clone(), logs.clone());

    let mut scope = Scope::new();
    scope.push_dynamic(
        "ARGS",
        Dynamic::from_map(metadata_to_rhai_map(&action.args)),
    );
    scope.push("TASK_ID", task.id.clone());
    scope.push("TARGET", task.target.clone());
    if let Some(comment) = &task.comment {
        scope.push("COMMENT", comment.clone());
    }

    let eval_result = engine.eval_with_scope::<Dynamic>(&mut scope, &source);
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
                *document = shared_doc.borrow().clone();
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

fn sandboxed_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_module_resolver(DummyModuleResolver::new());
    engine.set_max_operations(SCRIPT_MAX_OPERATIONS);
    engine.set_max_call_levels(SCRIPT_MAX_CALL_DEPTH);
    engine
}

fn register_script_api(
    engine: &mut Engine,
    document: Rc<RefCell<KeymapDocument>>,
    logs: Rc<RefCell<Vec<String>>>,
) {
    let doc_binding = Rc::clone(&document);
    engine.register_fn(
        "set_binding",
        move |layer: &str, index: INT, binding: &str| -> Result<(), Box<EvalAltResult>> {
            if index < 0 {
                return Err(script_error("binding index must be non-negative"));
            }
            doc_binding
                .borrow_mut()
                .set_binding(layer, index as usize, binding)
                .map_err(|err| script_error(err.to_string()))
        },
    );

    let doc_layer = Rc::clone(&document);
    engine.register_fn(
        "set_layer",
        move |layer: &str, bindings: RhaiArray| -> Result<(), Box<EvalAltResult>> {
            let normalized = array_to_string_vec(&bindings).map_err(|err| script_error(err))?;
            doc_layer
                .borrow_mut()
                .set_layer_bindings(layer, &normalized)
                .map_err(|err| script_error(err.to_string()))
        },
    );

    let doc_layer_metadata = Rc::clone(&document);
    engine.register_fn(
        "set_layer_metadata",
        move |layer: &str, metadata: RhaiMap| -> Result<(), Box<EvalAltResult>> {
            let map = rhai_map_to_metadata(&metadata).map_err(|err| script_error(err))?;
            let props = metadata_properties(&map);
            doc_layer_metadata
                .borrow_mut()
                .set_layer_metadata(layer, &props)
                .map_err(|err| script_error(err.to_string()))
        },
    );

    let doc_combo = Rc::clone(&document);
    engine.register_fn(
        "upsert_combo",
        move |name: &str,
              key_positions: RhaiArray,
              binding: &str|
              -> Result<(), Box<EvalAltResult>> {
            let positions = array_to_u32_vec(&key_positions).map_err(|err| script_error(err))?;
            let empty_layers: [u32; 0] = [];
            let empty_conditions: Vec<String> = Vec::new();
            doc_combo
                .borrow_mut()
                .upsert_combo(
                    name,
                    binding,
                    &positions,
                    None,
                    &empty_layers,
                    empty_conditions.as_slice(),
                )
                .map_err(|err| script_error(err.to_string()))
        },
    );

    let doc_combo_full = Rc::clone(&document);
    engine.register_fn(
        "upsert_combo_full",
        move |name: &str,
              key_positions: RhaiArray,
              binding: &str,
              timeout_ms: Dynamic,
              layers: RhaiArray,
              conditions: RhaiArray|
              -> Result<(), Box<EvalAltResult>> {
            let positions = array_to_u32_vec(&key_positions).map_err(|err| script_error(err))?;
            let timeout = parse_optional_u32(&timeout_ms).map_err(|err| script_error(err))?;
            let layer_indexes = {
                let borrowed = doc_combo_full.borrow();
                array_to_layer_indexes(&borrowed, &layers).map_err(|err| script_error(err))?
            };
            let condition_list =
                array_to_string_vec(&conditions).map_err(|err| script_error(err))?;
            doc_combo_full
                .borrow_mut()
                .upsert_combo(
                    name,
                    binding,
                    &positions,
                    timeout,
                    &layer_indexes,
                    &condition_list,
                )
                .map_err(|err| script_error(err.to_string()))
        },
    );

    let doc_layer_order = Rc::clone(&document);
    engine.register_fn(
        "move_layer",
        move |layer: &str, index: INT| -> Result<(), Box<EvalAltResult>> {
            if index < 0 {
                return Err(script_error("layer index must be non-negative"));
            }
            doc_layer_order
                .borrow_mut()
                .reorder_layer(layer, index as usize)
                .map_err(|err| script_error(err.to_string()))
        },
    );

    let log_sink = Rc::clone(&logs);
    engine.register_fn("log", move |message: &str| {
        log_sink.borrow_mut().push(message.to_string());
    });
}

fn metadata_to_rhai_map(metadata: &MetadataMap) -> RhaiMap {
    let mut map = RhaiMap::new();
    for (key, value) in metadata {
        map.insert(key.clone().into(), toml_to_dynamic(value));
    }
    map
}

fn rhai_map_to_metadata(map: &RhaiMap) -> Result<MetadataMap, String> {
    let mut result = MetadataMap::new();
    for (key, value) in map {
        let toml = dynamic_to_toml(value)?;
        result.insert(key.to_string(), toml);
    }
    Ok(result)
}

fn script_error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

fn toml_to_dynamic(value: &TomlValue) -> Dynamic {
    match value {
        TomlValue::String(text) => Dynamic::from(text.clone()),
        TomlValue::Integer(num) => Dynamic::from(*num),
        TomlValue::Float(num) => Dynamic::from(*num),
        TomlValue::Boolean(flag) => Dynamic::from(*flag),
        TomlValue::Array(items) => {
            let array = items.iter().map(toml_to_dynamic).collect::<RhaiArray>();
            Dynamic::from_array(array)
        }
        TomlValue::Table(entries) => {
            let mut map = RhaiMap::new();
            for (key, entry) in entries {
                map.insert(key.clone().into(), toml_to_dynamic(entry));
            }
            Dynamic::from_map(map)
        }
        TomlValue::Datetime(dt) => Dynamic::from(dt.to_string()),
    }
}

fn dynamic_to_toml(value: &Dynamic) -> Result<TomlValue, String> {
    if let Some(text) = value.clone().try_cast::<String>() {
        return Ok(TomlValue::String(text));
    }
    if let Some(flag) = value.clone().try_cast::<bool>() {
        return Ok(TomlValue::Boolean(flag));
    }
    if let Some(number) = value.clone().try_cast::<INT>() {
        return Ok(TomlValue::Integer(number as i64));
    }
    if let Some(number) = value.clone().try_cast::<FLOAT>() {
        return Ok(TomlValue::Float(number));
    }
    if let Some(array) = value.clone().try_cast::<RhaiArray>() {
        let mut items = Vec::with_capacity(array.len());
        for entry in &array {
            items.push(dynamic_to_toml(entry)?);
        }
        return Ok(TomlValue::Array(items));
    }
    if let Some(map) = value.clone().try_cast::<RhaiMap>() {
        let mut entries = TomlMap::new();
        for (key, entry) in map {
            entries.insert(key.to_string(), dynamic_to_toml(&entry)?);
        }
        return Ok(TomlValue::Table(entries));
    }
    Err("unsupported value in script metadata".to_string())
}

fn array_to_string_vec(values: &RhaiArray) -> Result<Vec<String>, String> {
    values
        .iter()
        .map(|value| {
            value
                .clone()
                .into_string()
                .map_err(|_| "array entry must be a string".to_string())
        })
        .collect()
}

fn array_to_u32_vec(values: &RhaiArray) -> Result<Vec<u32>, String> {
    values
        .iter()
        .map(|value| {
            let number = value
                .clone()
                .try_cast::<INT>()
                .ok_or_else(|| "array entry must be an integer".to_string())?;
            if number < 0 {
                return Err("array entry must be non-negative".to_string());
            }
            Ok(number as u32)
        })
        .collect()
}

fn parse_optional_u32(value: &Dynamic) -> Result<Option<u32>, String> {
    if value.is::<()>() {
        return Ok(None);
    }
    if let Some(number) = value.clone().try_cast::<INT>() {
        if number < 0 {
            return Err("value must be non-negative".into());
        }
        return Ok(Some(number as u32));
    }
    Ok(None)
}

fn resolve_script_path(root: Option<&Path>, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else if let Some(dir) = root {
        dir.join(candidate)
    } else {
        candidate.to_path_buf()
    }
}

fn parse_override_path(path: &str) -> Result<(String, usize), String> {
    const LAYERS_PREFIX: &str = "layers.";
    const BINDINGS_SEGMENT: &str = ".bindings";
    if !path.starts_with(LAYERS_PREFIX) {
        return Err(format!(
            "override path `{}` must start with `layers.`",
            path
        ));
    }
    let rest = &path[LAYERS_PREFIX.len()..];
    let bindings_idx = rest
        .find(BINDINGS_SEGMENT)
        .ok_or_else(|| format!("override path `{}` missing `.bindings`", path))?;
    let layer = rest[..bindings_idx].trim();
    if layer.is_empty() {
        return Err(format!("override path `{}` missing layer name", path));
    }
    let index_part = rest[bindings_idx + BINDINGS_SEGMENT.len()..].trim();
    if !index_part.starts_with('[') || !index_part.ends_with(']') {
        return Err(format!(
            "override path `{}` must include `[index]` after `.bindings`",
            path
        ));
    }
    let index_str = &index_part[1..index_part.len() - 1];
    let index = index_str
        .parse::<usize>()
        .map_err(|_| format!("override path `{}` has invalid index", path))?;
    Ok((layer.to_string(), index))
}

fn normalize_binding(parser: &mut BindingParser, value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err("binding value cannot be empty".into());
    }
    Ok(parser.parse_with_behavior_rules(value).to_binding_string())
}

fn normalize_binding_list(
    parser: &mut BindingParser,
    bindings: &[String],
) -> Result<Vec<String>, String> {
    if bindings.is_empty() {
        return Err("layer must define at least one binding".into());
    }
    bindings
        .iter()
        .map(|binding| normalize_binding(parser, binding))
        .collect()
}

fn layer_snapshot(document: &DtsDocument, layer: &str) -> Option<String> {
    let node = find_layer_node(&document.items, layer)?;
    let prop = find_bindings_property(node)?;
    let bindings = parse_binding_list(&prop.value.raw);
    if bindings.is_empty() {
        None
    } else {
        Some(bindings.join(" "))
    }
}

fn combo_snapshot(document: &DtsDocument, combo: &str) -> Option<String> {
    let combos_root = find_layer_node(&document.items, "combos")?;
    let combo_node = find_child_node(combos_root, combo)?;
    let mut parts = Vec::new();
    if let Some(prop) = combo_node
        .properties
        .iter()
        .find(|prop| prop.name == "key-positions")
    {
        parts.push(format!("key-positions={}", prop.value.raw.trim()));
    }
    if let Some(prop) = combo_node
        .properties
        .iter()
        .find(|prop| prop.name == "bindings")
    {
        parts.push(format!("bindings={}", prop.value.raw.trim()));
    }
    if let Some(prop) = combo_node
        .properties
        .iter()
        .find(|prop| prop.name == "timeout-ms")
    {
        parts.push(format!("timeout-ms={}", prop.value.raw.trim()));
    }
    if let Some(prop) = combo_node
        .properties
        .iter()
        .find(|prop| prop.name == "layers")
    {
        parts.push(format!("layers={}", prop.value.raw.trim()));
    }
    let conditions = combo_condition_comments(combo_node);
    if !conditions.is_empty() {
        parts.push(format!("conditions={}", conditions.join(" && ")));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("combo:{}:{}", combo, parts.join(";")))
    }
}

fn combo_condition_comments(node: &DtNode) -> Vec<String> {
    node.leading_comments
        .iter()
        .filter_map(|comment| extract_condition_comment(&comment.text))
        .collect()
}

fn extract_condition_comment(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with(COMBO_CONDITION_COMMENT_PREFIX) {
        return None;
    }
    let body = trimmed[COMBO_CONDITION_COMMENT_PREFIX.len()..].trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

fn layer_order_snapshot(document: &DtsDocument) -> String {
    if let Some(keymap) = find_layer_node(&document.items, "keymap") {
        let mut names = Vec::new();
        for item in &keymap.children {
            if let DtItem::Node(node) = item {
                if find_bindings_property(node).is_some() {
                    names.push(node.name.clone());
                }
            }
        }
        names.join(",")
    } else {
        String::new()
    }
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

fn resolve_combo_layers(
    document: &KeymapDocument,
    selectors: &[LayerSelector],
) -> Result<Vec<u32>, String> {
    if selectors.is_empty() {
        return Ok(Vec::new());
    }
    let names = document.layer_names();
    let mut name_to_index = HashMap::new();
    for (index, name) in names.iter().enumerate() {
        name_to_index.insert(name.clone(), index as u32);
    }
    let mut result = Vec::with_capacity(selectors.len());
    for selector in selectors {
        match selector {
            LayerSelector::Index(idx) => {
                if (*idx as usize) < names.len() {
                    result.push(*idx);
                } else {
                    return Err(format!(
                        "layer index {} out of range (len {})",
                        idx,
                        names.len()
                    ));
                }
            }
            LayerSelector::Name(name) => {
                if let Some(index) = name_to_index.get(name) {
                    result.push(*index);
                } else {
                    return Err(format!("layer `{}` not found for combo", name));
                }
            }
        }
    }
    Ok(result)
}

fn array_to_layer_indexes(
    document: &KeymapDocument,
    values: &RhaiArray,
) -> Result<Vec<u32>, String> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let names = document.layer_names();
    let mut lookup = HashMap::new();
    for (index, name) in names.iter().enumerate() {
        lookup.insert(name.clone(), index as u32);
    }
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        if let Some(index) = value.clone().try_cast::<INT>() {
            if index < 0 {
                return Err("layer index must be non-negative".into());
            }
            if (index as usize) >= names.len() {
                return Err(format!(
                    "layer index {} out of range (len {})",
                    index,
                    names.len()
                ));
            }
            result.push(index as u32);
        } else if let Some(name) = value.clone().try_cast::<String>() {
            if let Some(idx) = lookup.get(&name) {
                result.push(*idx);
            } else {
                return Err(format!("layer `{}` not found", name));
            }
        } else {
            return Err("layer reference must be a name or index".into());
        }
    }
    Ok(result)
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

fn apply_provider_error(outcome: &mut TaskOutcome, err: ProviderError) {
    outcome.status = TaskStatus::Error;
    append_message(&mut outcome.message, err.to_string());
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

fn find_bindings_property(node: &DtNode) -> Option<&DtProperty> {
    node.properties.iter().find(|prop| prop.name == "bindings")
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

fn metadata_properties(metadata: &MetadataMap) -> Vec<(String, String)> {
    metadata
        .iter()
        .map(|(key, value)| (key.clone(), format_metadata_value(value)))
        .collect()
}

fn format_metadata_value(value: &TomlValue) -> String {
    match value {
        TomlValue::String(text) => format!("\"{}\"", escape_string(text)),
        TomlValue::Integer(num) => num.to_string(),
        TomlValue::Float(num) => num.to_string(),
        TomlValue::Boolean(flag) => {
            if *flag {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        TomlValue::Array(items) => format_metadata_array(items),
        TomlValue::Table(entries) => {
            let body = entries
                .iter()
                .map(|(key, value)| format!("{} = {}", key, format_metadata_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {} }}", body)
        }
        TomlValue::Datetime(dt) => format!("\"{}\"", dt),
    }
}

fn format_metadata_array(items: &[TomlValue]) -> String {
    if items.is_empty() {
        "< >".to_string()
    } else if items
        .iter()
        .all(|item| matches!(item, TomlValue::Integer(_) | TomlValue::Float(_)))
    {
        let values = items
            .iter()
            .map(|item| format_metadata_value(item))
            .collect::<Vec<_>>();
        format!("< {} >", values.join(" "))
    } else {
        items
            .iter()
            .map(|item| match item {
                TomlValue::String(text) => format!("\"{}\"", escape_string(text)),
                _ => format_metadata_value(item),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn escape_string(input: &str) -> String {
    input.replace('"', "\\\"")
}

fn parse_binding_list(raw: &str) -> Vec<String> {
    parse_binding_groups(raw)
}

fn parse_binding_groups(raw: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for ch in raw.chars() {
        match ch {
            '<' => {
                depth += 1;
            }
            '>' => {
                if depth > 0 {
                    depth -= 1;
                }
                if depth == 0 && !current.is_empty() {
                    let trimmed = current.trim();
                    if !trimmed.is_empty() {
                        if trimmed.matches('&').count() > 1 {
                            groups.extend(split_binding_sequence(trimmed));
                        } else {
                            groups.push(trimmed.to_string());
                        }
                    }
                    current.clear();
                } else if depth > 0 {
                    current.push(ch);
                }
            }
            _ => {
                if depth > 0 {
                    current.push(ch);
                }
            }
        }
    }
    if groups.is_empty() {
        let trimmed = raw
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim_end_matches(';')
            .trim();
        if !trimmed.is_empty() {
            if trimmed.matches('&').count() > 1 {
                groups.extend(split_binding_sequence(trimmed));
            } else {
                groups.push(trimmed.to_string());
            }
        }
    }
    groups
}

fn split_binding_sequence(sequence: &str) -> Vec<String> {
    let mut bindings = Vec::new();
    let mut current = String::new();
    for token in sequence.split_whitespace() {
        if token.starts_with('&') {
            if !current.is_empty() {
                bindings.push(current.trim().to_string());
                current.clear();
            }
            current.push_str(token);
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(token);
        }
    }
    if !current.is_empty() {
        bindings.push(current.trim().to_string());
    }
    bindings
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    Prompt,
    Override,
    Skip,
    Script,
}

impl Default for ConflictPolicy {
    fn default() -> Self {
        ConflictPolicy::Prompt
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    Override,
    Combo,
    Layer,
    LayerOrder,
    Behavior,
    Meta,
    Script,
}

#[derive(Debug, Error)]
pub enum TaskConfigError {
    #[error("failed to parse task file: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("missing config section")]
    MissingConfig,
    #[error("missing config.format_version")]
    MissingFormatVersion,
    #[error("no tasks defined")]
    NoTasks,
    #[error("duplicate task id '{0}'")]
    DuplicateId(String),
    #[error("duplicate target '{0}'")]
    DuplicateTarget(String),
    #[error("target '{new}' overlaps with '{existing}'")]
    OverlappingTarget { existing: String, new: String },
    #[error("missing field '{field}' for task #{index} ({kind:?})")]
    MissingField {
        field: &'static str,
        kind: TaskKind,
        index: usize,
    },
    #[error("invalid field '{field}' for task #{index} ({kind:?}): {message}")]
    InvalidField {
        field: &'static str,
        kind: TaskKind,
        index: usize,
        message: String,
    },
}

#[derive(Deserialize)]
struct RawTaskFile {
    #[serde(default)]
    base: Option<RawBaseSection>,
    config: Option<RawConfigSection>,
    #[serde(default)]
    tasks: Vec<RawTask>,
}

#[derive(Deserialize, Default)]
struct RawBaseSection {
    template: Option<String>,
    version: Option<String>,
    #[serde(default)]
    metadata: MetadataMap,
}

impl From<RawBaseSection> for BaseSection {
    fn from(value: RawBaseSection) -> Self {
        BaseSection {
            template: value.template,
            version: value.version,
            metadata: value.metadata,
        }
    }
}

#[derive(Deserialize)]
struct RawConfigSection {
    format_version: Option<String>,
    #[serde(default)]
    default_conflict: Option<ConflictPolicy>,
    conflict_script: Option<String>,
    comment: Option<String>,
}

impl TryFrom<RawConfigSection> for ConfigSection {
    type Error = TaskConfigError;

    fn try_from(value: RawConfigSection) -> Result<Self, Self::Error> {
        let format_version = value
            .format_version
            .ok_or(TaskConfigError::MissingFormatVersion)?;
        Ok(ConfigSection {
            format_version,
            default_conflict: value.default_conflict.unwrap_or(ConflictPolicy::Prompt),
            conflict_script: value.conflict_script,
            comment: value.comment,
        })
    }
}

#[derive(Deserialize)]
struct RawTask {
    id: Option<String>,
    #[serde(rename = "type")]
    kind: TaskKind,
    path: Option<String>,
    target: Option<String>,
    comment: Option<String>,
    conflict: Option<ConflictPolicy>,
    value: Option<TomlValue>,
    from: Option<String>,
    name: Option<String>,
    key_positions: Option<Vec<u32>>,
    binding: Option<String>,
    timeout_ms: Option<u32>,
    layers: Option<Vec<RawLayerSelector>>,
    conditions: Option<Vec<String>>,
    bindings: Option<Vec<String>>,
    metadata: Option<MetadataMap>,
    layer: Option<String>,
    position: Option<i64>,
    before: Option<String>,
    after: Option<String>,
    behavior: Option<String>,
    settings: Option<MetadataMap>,
    key: Option<String>,
    script: Option<String>,
    filename: Option<String>,
    args: Option<MetadataMap>,
    expected: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawLayerSelector {
    Index(i64),
    Name(String),
}

impl RawTask {
    fn into_action(mut self, index: usize) -> Result<TaskAction, TaskConfigError> {
        match self.kind {
            TaskKind::Override => {
                let path = take_nonempty_string(
                    &mut self.path,
                    self.kind,
                    "path",
                    index,
                    "path cannot be empty",
                )?;
                let value = self
                    .value
                    .take()
                    .ok_or_else(|| missing(self.kind, "value", index))?;
                let value = value.as_str().map(str::to_string).ok_or_else(|| {
                    invalid(self.kind, "value", index, "override value must be a string")
                })?;
                if value.trim().is_empty() {
                    return Err(invalid(
                        self.kind,
                        "value",
                        index,
                        "override value cannot be empty",
                    ));
                }
                Ok(TaskAction::Override(OverrideTask {
                    path,
                    value,
                    from: self.from,
                }))
            }
            TaskKind::Combo => {
                let name = take_nonempty_string(
                    &mut self.name,
                    self.kind,
                    "name",
                    index,
                    "combo name cannot be empty",
                )?;
                let key_positions = self
                    .key_positions
                    .take()
                    .ok_or_else(|| missing(self.kind, "key_positions", index))?;
                if key_positions.is_empty() {
                    return Err(invalid(
                        self.kind,
                        "key_positions",
                        index,
                        "combo must declare at least one key position",
                    ));
                }
                let binding = take_nonempty_string(
                    &mut self.binding,
                    self.kind,
                    "binding",
                    index,
                    "binding cannot be empty",
                )?;
                Ok(TaskAction::Combo(ComboTask {
                    name,
                    key_positions,
                    binding,
                    timeout_ms: self.timeout_ms,
                    layers: convert_layer_selectors(self.layers, self.kind, index)?,
                    conditions: self.conditions.unwrap_or_default(),
                }))
            }
            TaskKind::Layer => {
                let name = take_nonempty_string(
                    &mut self.name,
                    self.kind,
                    "name",
                    index,
                    "layer name cannot be empty",
                )?;
                let bindings = self
                    .bindings
                    .take()
                    .ok_or_else(|| missing(self.kind, "bindings", index))?;
                if bindings.is_empty() {
                    return Err(invalid(
                        self.kind,
                        "bindings",
                        index,
                        "layer must define at least one binding",
                    ));
                }
                Ok(TaskAction::Layer(LayerTask {
                    name,
                    bindings,
                    metadata: self.metadata.unwrap_or_default(),
                }))
            }
            TaskKind::LayerOrder => {
                let movement = resolve_layer_order(&self, index)?;
                let layer = take_nonempty_string(
                    &mut self.layer,
                    self.kind,
                    "layer",
                    index,
                    "layer cannot be empty",
                )?;
                Ok(TaskAction::LayerOrder(LayerOrderTask { layer, movement }))
            }
            TaskKind::Behavior => {
                let behavior = take_nonempty_string(
                    &mut self.behavior,
                    self.kind,
                    "behavior",
                    index,
                    "behavior name cannot be empty",
                )?;
                let settings = self
                    .settings
                    .take()
                    .ok_or_else(|| missing(self.kind, "settings", index))?;
                if settings.is_empty() {
                    return Err(invalid(
                        self.kind,
                        "settings",
                        index,
                        "behavior settings cannot be empty",
                    ));
                }
                Ok(TaskAction::Behavior(BehaviorTask { behavior, settings }))
            }
            TaskKind::Meta => {
                let key = take_nonempty_string(
                    &mut self.key,
                    self.kind,
                    "key",
                    index,
                    "meta key cannot be empty",
                )?;
                let value = self
                    .value
                    .ok_or_else(|| missing(self.kind, "value", index))?;
                Ok(TaskAction::Meta(MetaTask { key, value }))
            }
            TaskKind::Script => {
                let source = match (self.filename, self.script) {
                    (Some(file), None) => {
                        if file.trim().is_empty() {
                            return Err(invalid(
                                self.kind,
                                "filename",
                                index,
                                "script filename cannot be empty",
                            ));
                        }
                        ScriptSource::File(file)
                    }
                    (None, Some(inline)) => {
                        if inline.trim().is_empty() {
                            return Err(invalid(
                                self.kind,
                                "script",
                                index,
                                "inline script cannot be empty",
                            ));
                        }
                        ScriptSource::Inline(inline)
                    }
                    (Some(file), Some(inline)) => {
                        if file.trim().is_empty() {
                            if inline.trim().is_empty() {
                                return Err(invalid(
                                    self.kind,
                                    "filename|script",
                                    index,
                                    "script task must specify a filename or inline script",
                                ));
                            }
                            ScriptSource::Inline(inline)
                        } else {
                            ScriptSource::File(file)
                        }
                    }
                    (None, None) => {
                        return Err(missing(self.kind, "filename|script", index));
                    }
                };
                Ok(TaskAction::Script(ScriptTask {
                    source,
                    args: self.args.unwrap_or_default(),
                }))
            }
        }
    }
}

fn convert_layer_selectors(
    raw: Option<Vec<RawLayerSelector>>,
    kind: TaskKind,
    index: usize,
) -> Result<Vec<LayerSelector>, TaskConfigError> {
    let Some(values) = raw else {
        return Ok(Vec::new());
    };
    let mut selectors = Vec::with_capacity(values.len());
    for value in values {
        selectors.push(value.into_selector(kind, index)?);
    }
    Ok(selectors)
}

impl RawLayerSelector {
    fn into_selector(self, kind: TaskKind, index: usize) -> Result<LayerSelector, TaskConfigError> {
        match self {
            RawLayerSelector::Index(value) => {
                if value < 0 {
                    Err(invalid(
                        kind,
                        "layers",
                        index,
                        "layer index must be non-negative",
                    ))
                } else {
                    Ok(LayerSelector::Index(value as u32))
                }
            }
            RawLayerSelector::Name(name) => {
                if name.trim().is_empty() {
                    Err(invalid(kind, "layers", index, "layer name cannot be empty"))
                } else {
                    Ok(LayerSelector::Name(name.trim().to_string()))
                }
            }
        }
    }
}

fn take_nonempty_string(
    slot: &mut Option<String>,
    kind: TaskKind,
    field: &'static str,
    index: usize,
    empty_message: &'static str,
) -> Result<String, TaskConfigError> {
    match slot.take() {
        Some(value) if !value.trim().is_empty() => Ok(value),
        Some(_) => Err(invalid(kind, field, index, empty_message)),
        None => Err(missing(kind, field, index)),
    }
}

fn missing(kind: TaskKind, field: &'static str, index: usize) -> TaskConfigError {
    TaskConfigError::MissingField { field, kind, index }
}

fn invalid(
    kind: TaskKind,
    field: &'static str,
    index: usize,
    message: impl Into<String>,
) -> TaskConfigError {
    TaskConfigError::InvalidField {
        field,
        kind,
        index,
        message: message.into(),
    }
}

fn resolve_layer_order(
    task: &RawTask,
    index: usize,
) -> Result<LayerOrderMovement, TaskConfigError> {
    let has_position = task.position.is_some();
    let has_before = task.before.is_some();
    let has_after = task.after.is_some();
    let count = has_position as u8 + has_before as u8 + has_after as u8;

    if count == 0 {
        return Err(missing(task.kind, "position|before|after", index));
    }
    if count > 1 {
        return Err(invalid(
            task.kind,
            "position|before|after",
            index,
            "specify only one of position/before/after",
        ));
    }

    if let Some(pos) = task.position {
        if pos < 0 {
            return Err(invalid(
                task.kind,
                "position",
                index,
                "position must be positive",
            ));
        }
        return Ok(LayerOrderMovement::Position(pos as usize));
    }
    if let Some(name) = &task.before {
        if name.trim().is_empty() {
            return Err(invalid(
                task.kind,
                "before",
                index,
                "layer reference cannot be empty",
            ));
        }
        return Ok(LayerOrderMovement::Before(name.clone()));
    }
    if let Some(name) = &task.after {
        if name.trim().is_empty() {
            return Err(invalid(
                task.kind,
                "after",
                index,
                "layer reference cannot be empty",
            ));
        }
        return Ok(LayerOrderMovement::After(name.clone()));
    }
    unreachable!("validated combination should ensure one branch returns")
}

fn auto_id(kind: TaskKind, target: &str, slug_counts: &mut HashMap<String, usize>) -> String {
    let base = slugify(&format!("{}-{}", kind.as_str(), target));
    let entry = slug_counts.entry(base.clone()).or_insert(0);
    *entry += 1;
    if *entry == 1 {
        base
    } else {
        format!("{}-{}", base, entry)
    }
}

fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str("task");
    }
    slug
}

fn normalize_locator(value: &str) -> String {
    value.split_whitespace().collect::<String>()
}

fn find_overlapping_target(existing: &[String], candidate: &str) -> Option<String> {
    for entry in existing {
        if targets_overlap(entry, candidate) {
            return Some(entry.clone());
        }
    }
    None
}

fn targets_overlap(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.starts_with(b) {
        return has_boundary(a, b.len());
    }
    if b.starts_with(a) {
        return has_boundary(b, a.len());
    }
    false
}

fn has_boundary(text: &str, prefix_len: usize) -> bool {
    if text.len() == prefix_len {
        return true;
    }
    text[prefix_len..]
        .chars()
        .next()
        .map(|ch| matches!(ch, '.' | '['))
        .unwrap_or(false)
}

impl TaskKind {
    fn as_str(self) -> &'static str {
        match self {
            TaskKind::Override => "override",
            TaskKind::Combo => "combo",
            TaskKind::Layer => "layer",
            TaskKind::LayerOrder => "layer-order",
            TaskKind::Behavior => "behavior",
            TaskKind::Meta => "meta",
            TaskKind::Script => "script",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
filename = "custom.rhai"
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
                ScriptSource::File(path) => assert_eq!(path, "custom.rhai"),
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
        let updated = layer_snapshot(exec.document.document(), "base").expect("layer snapshot");
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
        assert!(combo_snapshot(exec.document.document(), "combo_new").is_some());
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
        let snapshot = combo_snapshot(exec.document.document(), "combo_new").unwrap();
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

        let combos_root =
            find_layer_node(&exec.document.document().items, "combos").expect("combos node");
        let combo_node = find_child_node(combos_root, "combo_cond").expect("combo node");
        let comments: Vec<_> = combo_node
            .leading_comments
            .iter()
            .map(|comment| comment.text.trim().to_string())
            .collect();
        assert!(
            comments
                .iter()
                .any(|text| text.contains("layer_state == base"))
        );
        assert!(comments.iter().any(|text| text.contains("mods.shift")));

        let snapshot = combo_snapshot(exec.document.document(), "combo_cond").unwrap();
        assert!(
            snapshot.contains("conditions=layer_state == base && mods.shift"),
            "snapshot missing conditions: {}",
            snapshot
        );
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
        assert_eq!(layer_order_snapshot(exec.document.document()), "nav,base");
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
        assert_eq!(layer_order_snapshot(exec.document.document()), "nav,base");
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

        let layer = find_layer_node(&exec.document.document().items, "base").expect("layer node");
        let display_name = layer
            .properties
            .iter()
            .find(|prop| prop.name == "display_name")
            .expect("display_name property");
        assert_eq!(display_name.value.raw, "\"Primary\"");
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
        let updated = layer_snapshot(exec.document.document(), "base").expect("layer snapshot");
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
                source: ScriptSource::File("script_task_file.rhai".into()),
                args: MetadataMap::new(),
            }),
        });

        let base_doc = crate::dts::parse_str(sample_dts()).expect("parse dts");
        let document = KeymapDocument::from_document(base_doc);
        let exec = apply_tasks(document, &file);
        assert_eq!(exec.results[0].status, TaskStatus::Applied);
        let updated = layer_snapshot(exec.document.document(), "base").expect("layer snapshot");
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
set_layer_metadata("base", #{ display_name: "Primary", color: [1, 2, 3] });
set_layer("base", ["&kp ESC", "&kp W"]);
move_layer("nav", 0);
upsert_combo_full("combo_new", [0, 1], "&kp ENTER", 50, ["nav"], ["layer_state == nav"]);
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

        let layer = find_layer_node(&exec.document.document().items, "base").expect("layer");
        let display = layer
            .properties
            .iter()
            .find(|prop| prop.name == "display_name")
            .expect("display_name");
        assert_eq!(display.value.raw, "\"Primary\"");

        assert_eq!(layer_order_snapshot(exec.document.document()), "nav,base");

        let snapshot = combo_snapshot(exec.document.document(), "combo_new").unwrap();
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
        file.config.conflict_script = Some("conflict_override.rhai".into());
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
                        let counter = 0;
                        while true {
                            counter += 1;
                        }
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
                .contains("operations"),
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
"#
    }
}
