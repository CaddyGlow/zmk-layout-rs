//! Simplified adapter that exposes combo/behavior metadata for external tooling.

mod io;
mod layout;
mod listeners;
mod render;
mod template;
mod types;

pub use io::{
    export_standard_file, export_standard_file_with_template, export_standard_str,
    export_standard_str_with_template, export_standard_str_with_template_mode,
    import_standard_file, import_standard_file_with_template, import_standard_str,
    import_standard_str_with_template, render_standard_template,
};
pub use layout::AdapterLayout;
pub use template::{TemplateError, TemplateParseMode, template_contains_placeholders};
pub use types::{
    BehaviorSpec, ComboSpec, InputListenerNodeSpec, InputListenerSpec, InputProcessorSpec,
    LayerSpec, LayoutMetadata, MacroSpec,
};

use crate::{dts::DtsError, providers::ProviderError};
use thiserror::Error;

/// Errors surfaced by the adapter helpers.
#[derive(Debug, Error)]
pub enum AdapterError {
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Dts(#[from] DtsError),
    #[error(transparent)]
    Template(#[from] TemplateError),
}
