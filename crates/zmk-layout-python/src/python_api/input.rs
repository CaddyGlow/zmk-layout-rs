use std::cell::{Cell, RefCell};

use pyo3::prelude::*;

use super::util::{SharedLayout, ensure_staged, script_error};

/// Builder for configuring encoder/sensor inputs.
///
/// Use the fluent API to configure the input device, then call apply() to commit changes.
#[pyclass(unsendable)]
#[derive(Clone)]
pub struct InputObject {
    name: String,
    #[allow(dead_code)]
    layout: SharedLayout,
    input_type: RefCell<Option<String>>,
    cw: RefCell<Option<String>>,
    ccw: RefCell<Option<String>>,
    press: RefCell<Option<String>>,
    resolution: RefCell<Option<i64>>,
    applied: Cell<bool>,
}

impl InputObject {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        Self {
            name,
            layout,
            input_type: RefCell::new(None),
            cw: RefCell::new(None),
            ccw: RefCell::new(None),
            press: RefCell::new(None),
            resolution: RefCell::new(None),
            applied: Cell::new(false),
        }
    }

    fn apply_internal(&self) -> PyResult<()> {
        self.ensure_staged()?;
        // No-op placeholder until encoder/sensor plumbing exists.
        self.applied.set(true);
        Ok(())
    }

    fn ensure_staged(&self) -> PyResult<()> {
        ensure_staged(&self.applied, &format!("input '{}'", self.name))
    }
}

#[pymethods]
impl InputObject {
    /// Set the input type (e.g., "encoder", "sensor").
    ///
    /// Args:
    ///     value: The input type string.
    ///
    /// Returns:
    ///     Self for method chaining.
    #[pyo3(name = "type_")]
    fn set_type(&self, value: String) -> PyResult<Self> {
        self.ensure_staged()?;
        self.input_type.borrow_mut().replace(value);
        Ok(self.clone())
    }

    /// Set the binding for clockwise rotation.
    ///
    /// Args:
    ///     binding: The binding string for CW rotation.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn on_turn_cw(&self, binding: String) -> PyResult<Self> {
        self.ensure_staged()?;
        self.cw.borrow_mut().replace(binding);
        Ok(self.clone())
    }

    /// Set the binding for counter-clockwise rotation.
    ///
    /// Args:
    ///     binding: The binding string for CCW rotation.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn on_turn_ccw(&self, binding: String) -> PyResult<Self> {
        self.ensure_staged()?;
        self.ccw.borrow_mut().replace(binding);
        Ok(self.clone())
    }

    /// Set the binding for encoder press.
    ///
    /// Args:
    ///     binding: The binding string for press action.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn on_press(&self, binding: String) -> PyResult<Self> {
        self.ensure_staged()?;
        self.press.borrow_mut().replace(binding);
        Ok(self.clone())
    }

    /// Set the encoder resolution.
    ///
    /// Args:
    ///     value: The resolution value (must be non-negative).
    ///
    /// Returns:
    ///     Self for method chaining.
    fn resolution(&self, value: i64) -> PyResult<Self> {
        if value < 0 {
            return Err(script_error("resolution must be non-negative"));
        }
        self.ensure_staged()?;
        self.resolution.borrow_mut().replace(value);
        Ok(self.clone())
    }

    /// Get the input type.
    ///
    /// Returns:
    ///     The input type string or None.
    fn get_type(&self) -> Option<String> {
        self.input_type.borrow().clone()
    }

    /// Get the clockwise binding.
    ///
    /// Returns:
    ///     The CW binding string or None.
    fn get_cw_binding(&self) -> Option<String> {
        self.cw.borrow().clone()
    }

    /// Get the counter-clockwise binding.
    ///
    /// Returns:
    ///     The CCW binding string or None.
    fn get_ccw_binding(&self) -> Option<String> {
        self.ccw.borrow().clone()
    }

    /// Get the name of this input.
    ///
    /// Returns:
    ///     The input name.
    #[getter]
    fn name(&self) -> String {
        self.name.clone()
    }

    /// Apply the input configuration.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn apply(&self) -> PyResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}
