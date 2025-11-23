use mlua::{
    DebugEvent, Error as LuaError, Function as LuaFunction, HookTriggers, Integer as LuaInteger,
    Lua, Result as LuaResult, Table as LuaTable, Value as LuaValue,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};
use thiserror::Error;
use toml::{Value as TomlValue, map::Map as TomlMap};

use crate::{
    layout_engine::{LayoutEngine, MetadataMap},
    lua_api::api::install_layout_api,
    providers::KeymapDocument,
};

use super::config::{ScriptSource, Task, TaskFile};

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

#[derive(Clone, Copy)]
pub struct ScriptGlobals<'a> {
    pub task_id: Option<&'a str>,
    pub target: Option<&'a str>,
    pub comment: Option<&'a str>,
}

/// Execution limits for Lua scripts.
pub const SCRIPT_MAX_OPERATIONS: u64 = 100_000;
pub const SCRIPT_MAX_CALL_DEPTH: usize = 64;
pub const SCRIPT_HOOK_INTERVAL: u64 = 1_000;

#[derive(Debug)]
pub enum ScriptDecision {
    Override(Option<String>),
    Skip(String),
    Abort(String),
}

pub struct ScriptEnvironment<'a> {
    pub(crate) script_dir: Option<&'a Path>,
    pub(crate) conflict_script: Option<&'a str>,
}

impl<'a> ScriptEnvironment<'a> {
    pub fn new(file: &'a TaskFile) -> Self {
        Self {
            script_dir: file.script_dir.as_deref(),
            conflict_script: file.config.conflict_script.as_deref(),
        }
    }

    pub fn run_conflict_script(&self, task: &Task, reason: &str) -> Result<ScriptDecision, String> {
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

    pub fn load_source(&self, source: &ScriptSource) -> Result<String, String> {
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

pub(crate) fn set_script_globals(
    lua: &Lua,
    args: &MetadataMap,
    globals: ScriptGlobals<'_>,
) -> LuaResult<()> {
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

pub(crate) fn lua_table_to_metadata(table: LuaTable) -> LuaResult<MetadataMap> {
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

pub(crate) fn lua_value_to_toml(value: LuaValue) -> LuaResult<TomlValue> {
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

pub(crate) fn table_to_string_vec(table: LuaTable) -> LuaResult<Vec<String>> {
    let mut result = Vec::new();
    for value in table.sequence_values::<String>() {
        result.push(value?);
    }
    Ok(result)
}

pub(crate) fn table_to_u32_vec(table: LuaTable) -> LuaResult<Vec<u32>> {
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

pub(crate) fn script_layers_to_indexes(
    engine: &LayoutEngine,
    table: LuaTable,
) -> Result<Vec<u32>, String> {
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

pub(crate) fn value_to_optional_u32(value: LuaValue) -> LuaResult<Option<u32>> {
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

pub(crate) fn resolve_script_path(root: Option<&Path>, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else if let Some(dir) = root {
        dir.join(candidate)
    } else {
        candidate.to_path_buf()
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

fn parse_conflict_resolution(value: LuaValue) -> Result<ScriptDecision, String> {
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
        "override" => Ok(ScriptDecision::Override(message)),
        "skip" => Ok(ScriptDecision::Skip(
            message.unwrap_or_else(|| "skipped by script".into()),
        )),
        "abort" => Ok(ScriptDecision::Abort(
            message.unwrap_or_else(|| "aborted by script".into()),
        )),
        other => Err(format!("unknown conflict action `{}`", other)),
    }
}
