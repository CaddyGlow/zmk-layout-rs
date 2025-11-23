//! High-level helpers for mutating keymap documents.

mod behaviors;
mod combos;
pub mod format;
mod layers;
mod util;

pub use behaviors::{BehaviorDefinition, BehaviorProvider};
pub use combos::{ComboDefinition, ComboProvider};
pub use format::BindingFormat;
pub use layers::{KeymapDocument, KeymapProvider};

use thiserror::Error;

pub const COMBO_CONDITION_COMMENT_PREFIX: &str = "// zmk-task:condition";

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("layer `{0}` not found")]
    LayerNotFound(String),
    #[error("bindings property missing for layer `{0}`")]
    BindingsMissing(String),
    #[error("combos section missing from document")]
    CombosMissing,
    #[error("combo `{0}` not found")]
    ComboNotFound(String),
    #[error("behaviors section missing from document")]
    BehaviorsMissing,
    #[error("behavior `{0}` not found")]
    BehaviorNotFound(String),
    #[error("property `{property}` missing for `{context}`")]
    PropertyMissing { property: String, context: String },
    #[error("{0}")]
    InvalidBinding(String),
    #[error("binding index {index} out of range (len {len})")]
    BindingIndex { index: usize, len: usize },
}
