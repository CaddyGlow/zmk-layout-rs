use std::cell::{Cell, RefCell};
use std::sync::Arc;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use zmk_layout_core::layout_engine::LayoutEngine;

/// Thread-safe shared layout engine using Arc for Python's GIL-based threading.
pub type SharedLayout = Arc<RefCell<LayoutEngine>>;
pub type SharedLogs = Arc<RefCell<Vec<String>>>;

/// Create a Python runtime error from a string message.
pub fn script_error(message: impl Into<String>) -> PyErr {
    PyRuntimeError::new_err(message.into())
}

/// Ensure a builder hasn't been applied yet.
pub fn ensure_staged(applied: &Cell<bool>, context: &str) -> PyResult<()> {
    if applied.get() {
        Err(script_error(format!(
            "{context} already applied; re-acquire builder to edit"
        )))
    } else {
        Ok(())
    }
}

/// Convert 1-based Python index to 0-based Rust index.
/// Python users expect 1-based indexing for keyboard positions (matching Lua API).
pub fn require_positive_index(index: i64, context: &str) -> PyResult<usize> {
    if index < 1 {
        return Err(script_error(format!(
            "{} index must be >= 1 (1-based indexing)",
            context
        )));
    }
    Ok((index - 1) as usize)
}

/// Convert a Python list of strings to a Rust Vec<String>.
pub fn pylist_to_strings(list: &Bound<'_, pyo3::types::PyList>) -> PyResult<Vec<String>> {
    let mut result = Vec::new();
    for item in list.iter() {
        let s: String = item.extract()?;
        result.push(s);
    }
    Ok(result)
}

/// Convert a Python list of integers to a Rust Vec<u32>, adjusting for 1-based indexing.
pub fn pylist_to_u32(list: &Bound<'_, pyo3::types::PyList>) -> PyResult<Vec<u32>> {
    let mut result = Vec::new();
    for item in list.iter() {
        let num: i64 = item.extract()?;
        if num < 1 {
            return Err(script_error("positions must be >= 1 (1-based indexing)"));
        }
        result.push((num - 1) as u32);
    }
    Ok(result)
}

/// Convert an optional Python integer to Option<u32>.
pub fn pyvalue_to_optional_u32(value: Option<i64>) -> PyResult<Option<u32>> {
    match value {
        None => Ok(None),
        Some(num) => {
            if num < 0 {
                Err(script_error("value must be non-negative"))
            } else {
                Ok(Some(num as u32))
            }
        }
    }
}
