//! Adapter-facing helpers that translate between structured layout data and
//! the DTS providers.

pub mod standard;

pub use standard::{
    AdapterError, AdapterLayout, BehaviorSpec, ComboSpec, export_standard_file,
    export_standard_str, import_standard_file, import_standard_str,
};
