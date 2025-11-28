use std::cell::{Cell, RefCell};

use pyo3::prelude::*;

use super::util::{SharedLayout, ensure_staged};

/// Builder for configuring conditional layer logic.
///
/// Use the fluent API to configure the conditional, then call apply() to commit changes.
#[pyclass(unsendable)]
#[derive(Clone)]
pub struct ConditionalObject {
    name: String,
    #[allow(dead_code)]
    layout: SharedLayout,
    condition: RefCell<Option<String>>,
    then_layer: RefCell<Option<String>>,
    else_layer: RefCell<Option<String>>,
    applied: Cell<bool>,
}

impl ConditionalObject {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        Self {
            name,
            layout,
            condition: RefCell::new(None),
            then_layer: RefCell::new(None),
            else_layer: RefCell::new(None),
            applied: Cell::new(false),
        }
    }

    fn apply_internal(&self) -> PyResult<()> {
        self.ensure_staged()?;
        // No-op placeholder until conditional support is wired into the engine.
        self.applied.set(true);
        Ok(())
    }

    fn ensure_staged(&self) -> PyResult<()> {
        ensure_staged(&self.applied, &format!("conditional '{}'", self.name))
    }
}

#[pymethods]
impl ConditionalObject {
    /// Set the condition expression.
    ///
    /// Args:
    ///     expr: The condition expression string.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn condition(&self, expr: String) -> PyResult<Self> {
        self.ensure_staged()?;
        self.condition.borrow_mut().replace(expr);
        Ok(self.clone())
    }

    /// Set the layer to activate when condition is true.
    ///
    /// Args:
    ///     layer: The layer name.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn then_layer(&self, layer: String) -> PyResult<Self> {
        self.ensure_staged()?;
        self.then_layer.borrow_mut().replace(layer);
        Ok(self.clone())
    }

    /// Set the layer to activate when condition is false.
    ///
    /// Args:
    ///     layer: The layer name.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn else_layer(&self, layer: String) -> PyResult<Self> {
        self.ensure_staged()?;
        self.else_layer.borrow_mut().replace(layer);
        Ok(self.clone())
    }

    /// Get the condition expression.
    ///
    /// Returns:
    ///     The condition string or None.
    fn get_condition(&self) -> Option<String> {
        self.condition.borrow().clone()
    }

    /// Get the then-layer name.
    ///
    /// Returns:
    ///     The layer name or None.
    fn get_then_layer(&self) -> Option<String> {
        self.then_layer.borrow().clone()
    }

    /// Get the else-layer name.
    ///
    /// Returns:
    ///     The layer name or None.
    fn get_else_layer(&self) -> Option<String> {
        self.else_layer.borrow().clone()
    }

    /// Get the name of this conditional.
    ///
    /// Returns:
    ///     The conditional name.
    #[getter]
    fn name(&self) -> String {
        self.name.clone()
    }

    /// Apply the conditional configuration.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn apply(&self) -> PyResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}
