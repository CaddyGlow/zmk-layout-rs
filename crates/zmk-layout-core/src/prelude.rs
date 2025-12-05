//! Convenience re-exports for downstream crates and examples.
//!
//! The prelude is additive; existing module paths remain available.

pub use crate::{
    dts::DtsDocument,
    key_positions::{KeyPositionMap, NumericOnlyResolver, PositionResolver},
    keymap::KeymapDocument as StandardKeymapDocument,
    providers::{KeymapDocument as DtsKeymapDocument, KeymapProvider, ProviderError},
    tasks::{
        apply_tasks, apply_tasks_with_backend, ExecutionMode, NoScriptBackend, ScriptBackend,
        TaskEngineOptions, TaskExecution, TaskOutcome, TaskStatus,
    },
};
