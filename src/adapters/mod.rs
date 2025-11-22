//! Adapter-facing helpers that translate between structured layout data and
//! the DTS providers.

pub mod bundle;
pub mod moergo;
pub mod standard;

pub use standard::{
    AdapterError, AdapterLayout, BehaviorSpec, ComboSpec, RegexExtractionConfig, TemplateParseMode,
    export_standard_file, export_standard_file_with_template, export_standard_str,
    export_standard_str_with_regex_extractions, export_standard_str_with_template,
    export_standard_str_with_template_mode, import_standard_file, import_standard_file_for_profile,
    import_standard_file_with_template, import_standard_str, import_standard_str_with_template,
    render_standard_template, render_standard_template_for_profile, template_contains_placeholders,
};
