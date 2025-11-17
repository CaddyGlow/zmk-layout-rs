//! Adapter-facing helpers that translate between structured layout data and
//! the DTS providers.

pub mod standard;

pub use standard::{
    AdapterError, AdapterLayout, BehaviorSpec, ComboSpec, export_standard_file,
    export_standard_file_with_template, export_standard_str, export_standard_str_with_template,
    import_standard_file, import_standard_file_with_template, import_standard_str,
    import_standard_str_with_template,
};
