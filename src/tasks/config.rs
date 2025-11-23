// Re-export configuration types from the engine module to keep the public API stable
// while the config parsing remains co-located with execution logic.
pub use super::engine::{
    BaseSection, BehaviorTask, ComboTask, ConflictPolicy, ConfigSection, LayerOrderMovement,
    LayerOrderTask, LayerTask, MetaTask, OverrideTask, ScriptSource, ScriptTask, Task,
    TaskAction, TaskConfigError, TaskFile, TaskKind,
};
