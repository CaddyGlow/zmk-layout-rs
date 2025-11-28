//! Script backend trait for conflict resolution and script execution.

use super::config::{ScriptSource, Task};

/// Decision made by a conflict resolution script.
#[derive(Debug)]
pub enum ScriptDecision {
    Override(Option<String>),
    Skip(String),
    Abort(String),
}

/// Trait for script execution backends.
///
/// This trait allows the tasks engine to work with different script backends
/// (Lua, JavaScript, etc.) or no script support at all.
pub trait ScriptBackend {
    /// Run a conflict resolution script for the given task and reason.
    fn run_conflict_script(&self, task: &Task, reason: &str) -> Result<ScriptDecision, String>;

    /// Load script source code from a ScriptSource.
    fn load_source(&self, source: &ScriptSource) -> Result<String, String>;
}

/// A no-op script backend that returns errors for all script operations.
///
/// This is used when the core library is used without the Lua backend.
#[derive(Debug, Clone, Copy)]
pub struct NoScriptBackend;

impl ScriptBackend for NoScriptBackend {
    fn run_conflict_script(&self, _task: &Task, _reason: &str) -> Result<ScriptDecision, String> {
        Err("script backend not available (compile with lua feature)".to_string())
    }

    fn load_source(&self, _source: &ScriptSource) -> Result<String, String> {
        Err("script backend not available (compile with lua feature)".to_string())
    }
}
