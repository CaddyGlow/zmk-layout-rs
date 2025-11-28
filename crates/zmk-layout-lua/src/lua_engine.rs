use mlua::{
    DebugEvent, Error as LuaError, Function as LuaFunction, HookTriggers, Lua, Result as LuaResult,
    Table as LuaTable, Value as LuaValue,
};
use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};
use thiserror::Error;
use toml::Value as TomlValue;

use zmk_layout_core::{
    keymap::KeymapDocument,
    layout_engine::{LayoutEngine, MetadataMap},
    tasks::{ScriptDecision, ScriptSource, Task, TaskFile},
};

use crate::lua_api::api::install_layout_api;

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

    let log_sink = Rc::clone(&logs);
    globals.set(
        "log",
        lua.create_function(move |_, message: String| {
            log_sink.borrow_mut().push(message);
            Ok(())
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

/// Lua implementation of the ScriptBackend trait.
///
/// This backend provides Lua scripting support for task execution
/// and conflict resolution in the ZMK layout engine.
pub struct LuaScriptBackend<'a> {
    script_dir: Option<&'a Path>,
    conflict_script: Option<&'a str>,
}

impl<'a> LuaScriptBackend<'a> {
    /// Create a new Lua script backend with explicit configuration.
    pub fn new(script_dir: Option<&'a Path>, conflict_script: Option<&'a str>) -> Self {
        Self {
            script_dir,
            conflict_script,
        }
    }

    /// Create a Lua script backend from a TaskFile.
    pub fn from_task_file(file: &'a TaskFile) -> Self {
        Self {
            script_dir: file.script_dir.as_deref(),
            conflict_script: file.config.conflict_script.as_deref(),
        }
    }
}

impl<'a> zmk_layout_core::tasks::ScriptBackend for LuaScriptBackend<'a> {
    fn run_conflict_script(&self, task: &Task, reason: &str) -> Result<ScriptDecision, String> {
        let env = ScriptEnvironment {
            script_dir: self.script_dir,
            conflict_script: self.conflict_script,
        };
        env.run_conflict_script(task, reason)
    }

    fn load_source(&self, source: &ScriptSource) -> Result<String, String> {
        let env = ScriptEnvironment {
            script_dir: self.script_dir,
            conflict_script: self.conflict_script,
        };
        env.load_source(source)
    }
}

/// Apply tasks with Lua script support.
///
/// This is a convenience function that creates a LuaScriptBackend from the
/// provided TaskFile and applies all tasks with default options.
pub fn apply_tasks_with_lua(
    document: KeymapDocument,
    file: &TaskFile,
) -> zmk_layout_core::tasks::TaskExecution {
    let backend = LuaScriptBackend::from_task_file(file);
    zmk_layout_core::tasks::apply_tasks_with_backend(
        document,
        file,
        zmk_layout_core::tasks::TaskEngineOptions::default(),
        &backend,
    )
}
