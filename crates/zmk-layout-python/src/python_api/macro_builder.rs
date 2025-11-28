use std::cell::{Cell, RefCell};

use pyo3::prelude::*;

use super::util::{SharedLayout, ensure_staged, script_error};

/// Builder for configuring macros.
///
/// Use the fluent API to configure the macro actions, then call apply() to commit changes.
#[pyclass(unsendable)]
#[derive(Clone)]
pub struct MacroObject {
    name: String,
    #[allow(dead_code)]
    layout: SharedLayout,
    actions: RefCell<Vec<String>>,
    applied: Cell<bool>,
}

impl MacroObject {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        Self {
            name,
            layout,
            actions: RefCell::new(Vec::new()),
            applied: Cell::new(false),
        }
    }

    fn push_action(&self, action: String) -> PyResult<()> {
        self.ensure_staged()?;
        self.actions.borrow_mut().push(action);
        Ok(())
    }

    pub fn as_binding_string(&self) -> PyResult<String> {
        self.ensure_applied()?;
        Ok(format!("&{}", self.name))
    }

    fn apply_internal(&self) -> PyResult<()> {
        self.ensure_staged()?;
        // Macro application is a no-op for now; assumes underlying DTS already defines behavior.
        self.applied.set(true);
        Ok(())
    }

    fn ensure_staged(&self) -> PyResult<()> {
        ensure_staged(&self.applied, &format!("macro '{}'", self.name))
    }

    fn ensure_applied(&self) -> PyResult<()> {
        if self.applied.get() {
            Ok(())
        } else {
            self.apply_internal()
        }
    }
}

#[pymethods]
impl MacroObject {
    /// Add a key press action.
    ///
    /// Args:
    ///     keys: The key(s) to press.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn press(&self, keys: &Bound<'_, PyAny>) -> PyResult<Self> {
        let repr = keys.repr()?.to_string();
        self.push_action(format!("press:{}", repr))?;
        Ok(self.clone())
    }

    /// Add a key release action.
    ///
    /// Args:
    ///     keys: The key(s) to release.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn release(&self, keys: &Bound<'_, PyAny>) -> PyResult<Self> {
        let repr = keys.repr()?.to_string();
        self.push_action(format!("release:{}", repr))?;
        Ok(self.clone())
    }

    /// Add a key tap action (press and release).
    ///
    /// Args:
    ///     keys: The key(s) to tap.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn tap(&self, keys: &Bound<'_, PyAny>) -> PyResult<Self> {
        let repr = keys.repr()?.to_string();
        self.push_action(format!("tap:{}", repr))?;
        Ok(self.clone())
    }

    /// Add a wait/delay action.
    ///
    /// Args:
    ///     ms: Duration to wait in milliseconds.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn wait(&self, ms: i64) -> PyResult<Self> {
        if ms < 0 {
            return Err(script_error("wait duration must be non-negative"));
        }
        self.push_action(format!("wait:{ms}"))?;
        Ok(self.clone())
    }

    /// Add a wait-for-release action.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn wait_release(&self) -> PyResult<Self> {
        self.push_action("wait_release".into())?;
        Ok(self.clone())
    }

    /// Add a wait-for-tap action.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn wait_tap(&self) -> PyResult<Self> {
        self.push_action("wait_tap".into())?;
        Ok(self.clone())
    }

    /// Get all recorded actions.
    ///
    /// Returns:
    ///     List of action strings.
    fn get_actions(&self) -> Vec<String> {
        self.actions.borrow().clone()
    }

    /// Apply the macro definition.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn apply(&self) -> PyResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}
