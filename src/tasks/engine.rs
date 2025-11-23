//! Task configuration parser for the layout customization workflow.
//!
//! Phase 1 focuses on ingesting the TOML schema, validating structure,
//! auto-generating missing task identifiers, and enforcing per-task
//! targets so later phases can reason about conflicts.

pub use crate::layout_engine::MetadataMap;

use mlua::{
    DebugEvent, Error as LuaError, Function as LuaFunction, HookTriggers, Integer as LuaInteger,
    Lua, Result as LuaResult, Table as LuaTable, Value as LuaValue,
};
use serde::Deserialize;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};
use thiserror::Error;
use toml::{Value as TomlValue, map::Map as TomlMap};

use crate::{
    layout_engine::{LayerSelector, LayoutEngine, LayoutEngineError},
    lua_api::api::install_layout_api,
    providers::KeymapDocument,
};

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

/// Result of executing a standalone script.
#[derive(Debug)]
pub struct ScriptResult {
    pub document: KeymapDocument,
    pub logs: Vec<String>,
    pub error: Option<String>,
}

/// Error type for script execution.
#[derive(Debug, Error)]
pub enum ScriptExecutionError {
    #[error("failed to initialize scripting engine: {0}")]
    Engine(String),
}

/// Execute a Lua script directly against a document.
///
/// This function executes a standalone Lua script against a KeymapDocument
/// without requiring a TOML task file wrapper.
pub fn execute_script(
    document: KeymapDocument,
    script_source: &str,
    _script_dir: Option<&Path>,
) -> Result<ScriptResult, ScriptExecutionError> {
    let mut engine_layout = LayoutEngine::new(document);
    let shared_engine = Rc::new(RefCell::new(engine_layout.clone()));
    let logs = Rc::new(RefCell::new(Vec::new()));

    let lua = create_layout_lua(shared_engine.clone(), logs.clone())
        .map_err(|err| ScriptExecutionError::Engine(err.to_string()))?;
    set_script_globals(
        &lua,
        &MetadataMap::new(),
        ScriptGlobals {
            task_id: None,
            target: None,
            comment: None,
        },
    )
    .map_err(|err| ScriptExecutionError::Engine(err.to_string()))?;

    let execution = lua.load(script_source).set_name("script").exec();
    match execution {
        Ok(_) => {
            let log_messages = logs.borrow().clone();
            engine_layout = shared_engine.borrow().clone();
            Ok(ScriptResult {
                document: engine_layout.into_document(),
                logs: log_messages,
                error: None,
            })
        }
        Err(err) => {
            let log_messages = logs.borrow().clone();
            Ok(ScriptResult {
                document: engine_layout.into_document(),
                logs: log_messages,
                error: Some(format!("{err}")),
            })
        }
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

    let before_value = Some(bindings[slot].clone());
    outcome.before = before_value.clone();

    let normalized = match engine.normalize_binding(&action.value) {
        Ok(val) => val,
        Err(err) => {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    };

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
        return outcome;
    }

    if mode == ExecutionMode::Apply {
        if let Err(err) = engine.set_binding(&layer, slot, &normalized) {
            apply_engine_error(&mut outcome, err);
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
    engine: &mut LayoutEngine,
    task: &Task,
    action: &LayerTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    let before = engine.layer_to_string(&action.name);
    outcome.before = before.clone();

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
        return outcome;
    }

    let normalized = match engine.normalize_binding_list(&action.bindings) {
        Ok(values) => values,
        Err(err) => {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    };

    if mode == ExecutionMode::Apply {
        if let Err(err) = engine.set_layer_bindings(&action.name, &normalized) {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
        if !action.metadata.is_empty() {
            let metadata = LayoutEngine::metadata_to_properties(&action.metadata);
            if let Err(err) = engine.set_layer_metadata(&action.name, &metadata) {
                apply_engine_error(&mut outcome, err);
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
    engine: &mut LayoutEngine,
    task: &Task,
    action: &ComboTask,
    mode: ExecutionMode,
    scripts: &ScriptEnvironment,
) -> TaskOutcome {
    let mut outcome = TaskOutcome::new(task);
    outcome.before = engine.combo_to_string(&action.name);

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
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

    if mode == ExecutionMode::Apply {
        if let Err(err) = engine.upsert_combo(
            &action.name,
            &normalized_binding,
            &action.key_positions,
            action.timeout_ms,
            &layers,
            &action.conditions,
        ) {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    } else {
        append_message(
            &mut outcome.message,
            "dry-run: combo task recorded but not applied to document",
        );
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
    let mut outcome = TaskOutcome::new(task);
    let before = engine.layer_order_to_string();
    outcome.before = Some(before);

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
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

    if mode == ExecutionMode::Apply {
        if let Err(err) = engine.reorder_layer(&action.layer, target_index) {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    } else {
        append_message(
            &mut outcome.message,
            "dry-run: layer ordering not applied (reporting current order)",
        );
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
    let mut outcome = TaskOutcome::new(task);
    outcome.before = engine.behavior_to_string(&action.behavior);

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
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

    if mode == ExecutionMode::Apply {
        if let Err(err) = engine.set_behavior_settings(&action.behavior, &action.settings) {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    } else {
        append_message(
            &mut outcome.message,
            "dry-run: behavior settings not applied (reporting desired result)",
        );
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
    let mut outcome = TaskOutcome::new(task);
    outcome.before = engine.meta_to_string(&action.key);

    let before_snapshot = outcome.before.clone();
    if !ensure_expected_state(task, before_snapshot.as_deref(), &mut outcome, scripts) {
        return outcome;
    }

    if mode == ExecutionMode::Apply {
        if let Err(err) = engine.set_meta_entry(&action.key, &action.value) {
            apply_engine_error(&mut outcome, err);
            return outcome;
        }
    } else {
        append_message(
            &mut outcome.message,
            "dry-run: meta entry not applied (reporting desired result)",
        );
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

pub(crate) const SCRIPT_MAX_OPERATIONS: u64 = 100_000;
pub(crate) const SCRIPT_MAX_CALL_DEPTH: usize = 64;
pub(crate) const SCRIPT_HOOK_INTERVAL: u64 = 1_000;

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
        let lua = create_lua_with_limits()
            .map_err(|err| format!("failed to init conflict script: {err}"))?;
        lua.load(&source)
            .set_name("conflict")
            .exec()
            .map_err(|err| format!("conflict script execution failed: {err}"))?;
        let globals = lua.globals();
        let resolver: LuaFunction = globals
            .get("resolve")
            .map_err(|err| format!("conflict script must define resolve(): {err}"))?;
        let payload = build_conflict_payload(&lua, task, reason)
            .map_err(|err| format!("failed to build conflict payload: {err}"))?;
        let result = resolver
            .call::<_, LuaValue>(payload)
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

fn build_conflict_payload<'lua>(
    lua: &'lua Lua,
    task: &Task,
    reason: &str,
) -> LuaResult<LuaTable<'lua>> {
    let table = lua.create_table()?;
    table.set("id", task.id.as_str())?;
    table.set("target", task.target.as_str())?;
    table.set("reason", reason)?;
    if let Some(comment) = &task.comment {
        table.set("comment", comment.as_str())?;
    }
    if let Some(expected) = &task.expected {
        table.set("expected", expected.as_str())?;
    }
    Ok(table)
}

fn parse_conflict_resolution(value: LuaValue) -> Result<ConflictResolution, String> {
    let table = match value {
        LuaValue::Table(table) => table,
        other => {
            return Err(format!(
                "conflict script must return a table, got {}",
                other.type_name()
            ));
        }
    };
    let action: String = table
        .get("action")
        .map_err(|err| format!("conflict script missing `action`: {err}"))?;
    let message: Option<String> = table
        .get("message")
        .map_err(|err| format!("failed to read `message`: {err}"))?;
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

#[derive(Clone, Copy)]
struct ScriptGlobals<'a> {
    task_id: Option<&'a str>,
    target: Option<&'a str>,
    comment: Option<&'a str>,
}

pub(crate) fn create_layout_lua(
    layout: Rc<RefCell<LayoutEngine>>,
    logs: Rc<RefCell<Vec<String>>>,
) -> LuaResult<Lua> {
    let lua = Lua::new();
    configure_lua_limits(&lua)?;
    register_script_api(&lua, layout, logs)?;
    Ok(lua)
}

pub(crate) fn create_lua_with_limits() -> LuaResult<Lua> {
    let lua = Lua::new();
    configure_lua_limits(&lua)?;
    Ok(lua)
}

pub(crate) fn configure_lua_limits(lua: &Lua) -> LuaResult<()> {
    let operation_counter = Rc::new(Cell::new(0u64));
    let depth_counter = Rc::new(Cell::new(0i32));

    let operations = Rc::clone(&operation_counter);
    let depth = Rc::clone(&depth_counter);
    let triggers = HookTriggers::new()
        .on_calls()
        .on_returns()
        .every_nth_instruction(SCRIPT_HOOK_INTERVAL as u32);
    lua.set_hook(triggers, move |_, debug| {
        match debug.event() {
            DebugEvent::Call => {
                let current = depth.get() + 1;
                depth.set(current);
                if current as usize > SCRIPT_MAX_CALL_DEPTH {
                    return Err(script_error(format!(
                        "script exceeded call depth limit ({})",
                        SCRIPT_MAX_CALL_DEPTH
                    )));
                }
            }
            DebugEvent::Ret | DebugEvent::TailCall => {
                let current = depth.get().saturating_sub(1);
                depth.set(current);
            }
            DebugEvent::Count => {
                let total = operations.get().saturating_add(SCRIPT_HOOK_INTERVAL);
                operations.set(total);
                if total > SCRIPT_MAX_OPERATIONS {
                    return Err(script_error(format!(
                        "script exceeded instruction limit ({})",
                        SCRIPT_MAX_OPERATIONS
                    )));
                }
            }
            _ => {}
        }
        Ok(())
    });
    Ok(())
}

pub(crate) fn register_script_api(
    lua: &Lua,
    layout: Rc<RefCell<LayoutEngine>>,
    logs: Rc<RefCell<Vec<String>>>,
) -> LuaResult<()> {
    let globals = lua.globals();

    let doc_binding = Rc::clone(&layout);
    let set_binding = lua.create_function(
        move |_, (layer, index, binding): (String, LuaInteger, String)| {
            if index < 0 {
                return Err(script_error("binding index must be non-negative"));
            }
            let mut engine = doc_binding.borrow_mut();
            let normalized = engine
                .normalize_binding(&binding)
                .map_err(|err| script_error(err.to_string()))?;
            engine
                .set_binding(&layer, index as usize, &normalized)
                .map_err(|err| script_error(err.to_string()))
        },
    )?;
    globals.set("set_binding", set_binding)?;

    let doc_layer = Rc::clone(&layout);
    let set_layer = lua.create_function(move |_, (layer, bindings): (String, LuaTable)| {
        let desired = table_to_string_vec(bindings.clone())?;
        let mut engine = doc_layer.borrow_mut();
        let normalized = engine
            .normalize_binding_list(&desired)
            .map_err(|err| script_error(err.to_string()))?;
        engine
            .set_layer_bindings(&layer, &normalized)
            .map_err(|err| script_error(err.to_string()))
    })?;
    globals.set("set_layer", set_layer)?;

    let doc_layer_metadata = Rc::clone(&layout);
    let set_layer_metadata =
        lua.create_function(move |_, (layer, metadata): (String, LuaTable)| {
            let map = lua_table_to_metadata(metadata.clone())
                .map_err(|err| script_error(err.to_string()))?;
            let props = LayoutEngine::metadata_to_properties(&map);
            doc_layer_metadata
                .borrow_mut()
                .set_layer_metadata(&layer, &props)
                .map_err(|err| script_error(err.to_string()))
        })?;
    globals.set("set_layer_metadata", set_layer_metadata)?;

    let doc_combo = Rc::clone(&layout);
    let upsert_combo = lua.create_function(
        move |_, (name, key_positions, binding): (String, LuaTable, String)| {
            let positions = table_to_u32_vec(key_positions.clone())?;
            let mut engine = doc_combo.borrow_mut();
            let normalized = engine
                .normalize_binding(&binding)
                .map_err(|err| script_error(err.to_string()))?;
            engine
                .upsert_combo(&name, &normalized, &positions, None, &[], &[])
                .map_err(|err| script_error(err.to_string()))
        },
    )?;
    globals.set("upsert_combo", upsert_combo)?;

    let doc_combo_full = Rc::clone(&layout);
    let upsert_combo_full = lua.create_function(
        move |_,
              (name, key_positions, binding, timeout, layers, conditions): (
            String,
            LuaTable,
            String,
            LuaValue,
            LuaTable,
            LuaTable,
        )| {
            let positions = table_to_u32_vec(key_positions.clone())?;
            let timeout = value_to_optional_u32(timeout)?;
            let mut engine = doc_combo_full.borrow_mut();
            let normalized = engine
                .normalize_binding(&binding)
                .map_err(|err| script_error(err.to_string()))?;
            let layer_indexes =
                script_layers_to_indexes(&engine, layers.clone()).map_err(script_error)?;
            let condition_list = table_to_string_vec(conditions.clone())?;
            engine
                .upsert_combo(
                    &name,
                    &normalized,
                    &positions,
                    timeout,
                    &layer_indexes,
                    &condition_list,
                )
                .map_err(|err| script_error(err.to_string()))
        },
    )?;
    globals.set("upsert_combo_full", upsert_combo_full)?;

    let doc_layer_order = Rc::clone(&layout);
    let move_layer = lua.create_function(move |_, (layer, index): (String, LuaInteger)| {
        if index < 0 {
            return Err(script_error("layer index must be non-negative"));
        }
        doc_layer_order
            .borrow_mut()
            .reorder_layer(&layer, index as usize)
            .map_err(|err| script_error(err.to_string()))
    })?;
    globals.set("move_layer", move_layer)?;

    let log_sink = Rc::clone(&logs);
    globals.set(
        "log",
        lua.create_function(move |_, message: String| {
            log_sink.borrow_mut().push(message);
            Ok(())
        })?,
    )?;

    let doc_behavior = Rc::clone(&layout);
    let set_behavior_bindings =
        lua.create_function(move |_, (behavior, bindings): (String, LuaTable)| {
            let desired = table_to_string_vec(bindings.clone())?;
            if desired.is_empty() {
                return Err(script_error("behavior bindings cannot be empty"));
            }
            let mut engine = doc_behavior.borrow_mut();
            let mut normalized = Vec::with_capacity(desired.len());
            for binding in desired {
                normalized.push(
                    engine
                        .normalize_binding(&binding)
                        .map_err(|err| script_error(err.to_string()))?,
                );
            }
            engine
                .set_behavior_bindings(&behavior, &normalized)
                .map_err(|err| script_error(err.to_string()))
        })?;
    globals.set("set_behavior_bindings", set_behavior_bindings)?;

    let doc_behavior_settings = Rc::clone(&layout);
    let set_behavior_settings =
        lua.create_function(move |_, (behavior, settings): (String, LuaTable)| {
            let metadata = lua_table_to_metadata(settings.clone())
                .map_err(|err| script_error(err.to_string()))?;
            doc_behavior_settings
                .borrow_mut()
                .set_behavior_settings(&behavior, &metadata)
                .map_err(|err| script_error(err.to_string()))
        })?;
    globals.set("set_behavior_settings", set_behavior_settings)?;

    let doc_meta = Rc::clone(&layout);
    let set_meta = lua.create_function(move |_, (key, value): (String, LuaValue)| {
        let toml = lua_value_to_toml(value).map_err(|err| script_error(err.to_string()))?;
        doc_meta
            .borrow_mut()
            .set_meta_entry(&key, &toml)
            .map_err(|err| script_error(err.to_string()))
    })?;
    globals.set("set_meta", set_meta)?;

    let doc_add_layer = Rc::clone(&layout);
    let add_layer = lua.create_function(move |_, (name, bindings): (String, LuaTable)| {
        let binding_list = table_to_string_vec(bindings.clone())?;
        let mut engine = doc_add_layer.borrow_mut();
        let normalized = engine
            .normalize_binding_list(&binding_list)
            .map_err(|err| script_error(err.to_string()))?;
        engine
            .add_layer(&name, &normalized)
            .map_err(|err| script_error(err.to_string()))
    })?;
    globals.set("add_layer", add_layer)?;

    let doc_remove_layer = Rc::clone(&layout);
    globals.set(
        "remove_layer",
        lua.create_function(move |_, name: String| {
            doc_remove_layer
                .borrow_mut()
                .remove_layer(&name)
                .map_err(|err| script_error(err.to_string()))
        })?,
    )?;

    let doc_get_layer = Rc::clone(&layout);
    let get_layer = lua.create_function(move |lua_ctx, name: String| {
        let engine = doc_get_layer.borrow();
        match engine.get_layer(&name) {
            Some(info) => {
                let table = lua_ctx.create_table()?;
                table.set("name", info.name)?;
                table.set("index", info.index as LuaInteger)?;
                table.set("binding_count", info.binding_count as LuaInteger)?;
                let bindings_table = lua_ctx.create_table()?;
                for (idx, binding) in info.bindings.into_iter().enumerate() {
                    bindings_table.set(idx + 1, binding)?;
                }
                table.set("bindings", bindings_table)?;
                Ok(table)
            }
            None => Err(script_error(format!("layer '{}' not found", name))),
        }
    })?;
    globals.set("get_layer", get_layer)?;

    let doc_list_layers = Rc::clone(&layout);
    let list_layers = lua.create_function(move |lua_ctx, ()| {
        let engine = doc_list_layers.borrow();
        let layers = engine.list_layers();
        let result = lua_ctx.create_table()?;
        for (idx, info) in layers.into_iter().enumerate() {
            let entry = lua_ctx.create_table()?;
            entry.set("name", info.name)?;
            entry.set("index", info.index as LuaInteger)?;
            entry.set("binding_count", info.binding_count as LuaInteger)?;
            let bindings = lua_ctx.create_table()?;
            for (binding_index, binding) in info.bindings.into_iter().enumerate() {
                bindings.set(binding_index + 1, binding)?;
            }
            entry.set("bindings", bindings)?;
            result.set(idx + 1, entry)?;
        }
        Ok(result)
    })?;
    globals.set("list_layers", list_layers)?;

    let doc_layer_count = Rc::clone(&layout);
    globals.set(
        "layer_count",
        lua.create_function(move |_, ()| {
            Ok(doc_layer_count.borrow().layer_names().len() as LuaInteger)
        })?,
    )?;

    install_layout_api(lua, Rc::clone(&layout), Rc::clone(&logs))?;

    Ok(())
}

fn set_script_globals(lua: &Lua, args: &MetadataMap, globals: ScriptGlobals<'_>) -> LuaResult<()> {
    let table = metadata_to_lua_table(lua, args)?;
    let global_table = lua.globals();
    global_table.set("ARGS", table)?;
    if let Some(task_id) = globals.task_id {
        global_table.set("TASK_ID", task_id)?;
    }
    if let Some(target) = globals.target {
        global_table.set("TARGET", target)?;
    }
    if let Some(comment) = globals.comment {
        global_table.set("COMMENT", comment)?;
    }
    Ok(())
}

fn metadata_to_lua_table<'lua>(
    lua: &'lua Lua,
    metadata: &MetadataMap,
) -> LuaResult<LuaTable<'lua>> {
    let table = lua.create_table()?;
    for (key, value) in metadata {
        table.set(key.as_str(), toml_to_lua_value(lua, value)?)?;
    }
    Ok(table)
}

fn toml_to_lua_value<'lua>(lua: &'lua Lua, value: &TomlValue) -> LuaResult<LuaValue<'lua>> {
    Ok(match value {
        TomlValue::String(text) => LuaValue::String(lua.create_string(text)?),
        TomlValue::Integer(num) => LuaValue::Integer(*num),
        TomlValue::Float(num) => LuaValue::Number(*num),
        TomlValue::Boolean(flag) => LuaValue::Boolean(*flag),
        TomlValue::Array(items) => {
            let table = lua.create_table()?;
            for (idx, entry) in items.iter().enumerate() {
                table.set(idx + 1, toml_to_lua_value(lua, entry)?)?;
            }
            LuaValue::Table(table)
        }
        TomlValue::Table(entries) => {
            let table = lua.create_table()?;
            for (key, entry) in entries {
                table.set(key.as_str(), toml_to_lua_value(lua, entry)?)?;
            }
            LuaValue::Table(table)
        }
        TomlValue::Datetime(dt) => LuaValue::String(lua.create_string(&dt.to_string())?),
    })
}

fn lua_table_to_metadata(table: LuaTable) -> LuaResult<MetadataMap> {
    let mut result = MetadataMap::new();
    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (key, value) = pair?;
        let key = match key {
            LuaValue::String(name) => name.to_str()?.to_string(),
            other => {
                return Err(script_error(format!(
                    "metadata keys must be strings, found {}",
                    other.type_name()
                )));
            }
        };
        result.insert(key, lua_value_to_toml(value)?);
    }
    Ok(result)
}

fn lua_value_to_toml(value: LuaValue) -> LuaResult<TomlValue> {
    match value {
        LuaValue::String(text) => Ok(TomlValue::String(text.to_str()?.to_string())),
        LuaValue::Boolean(flag) => Ok(TomlValue::Boolean(flag)),
        LuaValue::Integer(num) => Ok(TomlValue::Integer(num)),
        LuaValue::Number(num) => Ok(TomlValue::Float(num)),
        LuaValue::Table(table) => {
            if table_represents_array(&table)? {
                let mut items = Vec::new();
                for entry in table.sequence_values::<LuaValue>() {
                    items.push(lua_value_to_toml(entry?)?);
                }
                Ok(TomlValue::Array(items))
            } else {
                let mut entries = TomlMap::new();
                for pair in table.pairs::<LuaValue, LuaValue>() {
                    let (key, entry) = pair?;
                    let key = match key {
                        LuaValue::String(name) => name.to_str()?.to_string(),
                        other => {
                            return Err(script_error(format!(
                                "metadata keys must be strings, found {}",
                                other.type_name()
                            )));
                        }
                    };
                    entries.insert(key, lua_value_to_toml(entry)?);
                }
                Ok(TomlValue::Table(entries))
            }
        }
        LuaValue::Nil => Err(script_error("metadata values cannot be nil")),
        other => Err(script_error(format!(
            "unsupported metadata value type `{}`",
            other.type_name()
        ))),
    }
}

fn table_represents_array(table: &LuaTable) -> LuaResult<bool> {
    for pair in table.clone().pairs::<LuaValue, LuaValue>() {
        let (key, _) = pair?;
        match key {
            LuaValue::Integer(index) if index >= 1 => continue,
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn table_to_string_vec(table: LuaTable) -> LuaResult<Vec<String>> {
    let mut result = Vec::new();
    for value in table.sequence_values::<String>() {
        result.push(value?);
    }
    Ok(result)
}

fn table_to_u32_vec(table: LuaTable) -> LuaResult<Vec<u32>> {
    let mut result = Vec::new();
    for value in table.sequence_values::<LuaInteger>() {
        let number = value?;
        if number < 0 {
            return Err(script_error("array entry must be non-negative"));
        }
        result.push(number as u32);
    }
    Ok(result)
}

fn script_layers_to_indexes(engine: &LayoutEngine, table: LuaTable) -> Result<Vec<u32>, String> {
    if table.is_empty() {
        return Ok(Vec::new());
    }
    let names = engine.layer_names();
    let mut lookup = HashMap::new();
    for (index, name) in names.iter().enumerate() {
        lookup.insert(name.clone(), index as u32);
    }
    let mut result = Vec::new();
    for entry in table.sequence_values::<LuaValue>() {
        let value = entry.map_err(|err| err.to_string())?;
        match value {
            LuaValue::Integer(idx) => {
                if idx < 0 {
                    return Err("layer index must be non-negative".into());
                }
                result.push(idx as u32);
            }
            LuaValue::String(name) => {
                let text = name.to_str().map_err(|err| err.to_string())?;
                if let Some(idx) = lookup.get(text) {
                    result.push(*idx);
                } else {
                    return Err(format!("layer `{}` not found", text));
                }
            }
            other => {
                return Err(format!(
                    "layer reference must be a string or integer (got {})",
                    other.type_name()
                ));
            }
        }
    }
    Ok(result)
}

fn value_to_optional_u32(value: LuaValue) -> LuaResult<Option<u32>> {
    match value {
        LuaValue::Nil => Ok(None),
        LuaValue::Integer(num) => {
            if num < 0 {
                Err(script_error("value must be non-negative"))
            } else {
                Ok(Some(num as u32))
            }
        }
        LuaValue::Number(num) => {
            if num < 0.0 {
                Err(script_error("value must be non-negative"))
            } else {
                Ok(Some(num as u32))
            }
        }
        other => Err(script_error(format!(
            "timeout must be a number or nil (got {})",
            other.type_name()
        ))),
    }
}

fn script_error(message: impl Into<String>) -> LuaError {
    LuaError::RuntimeError(message.into())
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

pub fn parse_override_path(path: &str) -> Result<(String, usize), String> {
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

pub fn normalize_locator(value: &str) -> String {
    value.split_whitespace().collect::<String>()
}

pub fn find_overlapping_target(existing: &[String], candidate: &str) -> Option<String> {
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
    use crate::ast::{DtItem, DtNode};
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
set_layer_metadata("base", { display_name = "Primary", color = {1, 2, 3} })
set_layer("base", {"&kp ESC", "&kp W"})
move_layer("nav", 0)
upsert_combo_full("combo_new", {0, 1}, "&kp ENTER", 50, {"nav"}, {"layer_state == nav"})
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
