#![forbid(unsafe_code)]

//! Lua scripting API for ZMK layout manipulation.

pub mod lua_api;
pub mod lua_engine;

// Re-export key types
pub use lua_api::api::{create_layout_api, install_layout_api};
pub use lua_engine::{
    apply_tasks_with_lua, execute_script, LuaScriptBackend, ScriptExecutionError, ScriptResult,
};
