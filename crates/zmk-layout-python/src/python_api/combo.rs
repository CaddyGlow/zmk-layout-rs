use std::cell::{Cell, RefCell};

use pyo3::prelude::*;
use pyo3::types::PyList;

use zmk_layout_core::layout_engine::LayerSelector;

use super::util::{
    SharedLayout, ensure_staged, pylist_to_strings, pylist_to_u32, pyvalue_to_optional_u32,
    script_error,
};

/// Builder for configuring key combos.
///
/// Use the fluent API to configure the combo, then call apply() to commit changes.
#[pyclass(unsendable)]
#[derive(Clone)]
pub struct ComboObject {
    name: String,
    layout: SharedLayout,
    keys: RefCell<Option<Vec<u32>>>,
    binding: RefCell<Option<String>>,
    timeout: RefCell<Option<Option<u32>>>,
    layers: RefCell<Vec<String>>,
    conditions: RefCell<Vec<String>>,
    applied: Cell<bool>,
}

impl ComboObject {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        let combo = Self {
            name,
            layout,
            keys: RefCell::new(None),
            binding: RefCell::new(None),
            timeout: RefCell::new(None),
            layers: RefCell::new(Vec::new()),
            conditions: RefCell::new(Vec::new()),
            applied: Cell::new(false),
        };
        combo.seed_from_layout();
        combo
    }

    pub fn as_binding_string(&self) -> PyResult<String> {
        self.ensure_applied()?;
        match &*self.binding.borrow() {
            Some(binding) => Ok(binding.clone()),
            None => Err(script_error(format!(
                "combo '{}' requires a binding before use",
                self.name
            ))),
        }
    }

    fn apply_internal(&self) -> PyResult<()> {
        self.ensure_staged()?;

        let keys = self
            .keys
            .borrow()
            .clone()
            .ok_or_else(|| script_error(format!("combo '{}' requires keys()", self.name)))?;
        let binding_value = self
            .binding
            .borrow()
            .clone()
            .ok_or_else(|| script_error(format!("combo '{}' requires binding()", self.name)))?;
        let timeout = self.timeout.borrow().clone().unwrap_or(None);
        let layers = self.layers.borrow().clone();
        let conditions = self.conditions.borrow().clone();

        let mut engine = self.layout.borrow_mut();
        let normalized = engine
            .normalize_binding(&binding_value)
            .map_err(|err| script_error(err.to_string()))?;
        let selectors: Vec<LayerSelector> = layers.into_iter().map(LayerSelector::Name).collect();
        let layer_indexes = engine
            .resolve_layer_selectors(&selectors)
            .map_err(|err| script_error(err.to_string()))?;
        engine
            .upsert_combo(
                &self.name,
                &normalized,
                &keys,
                timeout,
                &layer_indexes,
                &conditions,
            )
            .map_err(|err| script_error(err.to_string()))?;
        self.applied.set(true);
        Ok(())
    }

    fn seed_from_layout(&self) {
        let engine = self.layout.borrow();
        if let Some(state) = engine.combo_state(&self.name) {
            self.keys.borrow_mut().replace(state.key_positions);
            *self.binding.borrow_mut() = state.binding;
            *self.timeout.borrow_mut() = Some(state.timeout_ms);

            let layer_names = {
                let names = engine.layer_names();
                state
                    .layers
                    .iter()
                    .filter_map(|idx| names.get(*idx as usize).cloned())
                    .collect()
            };
            *self.layers.borrow_mut() = layer_names;
            *self.conditions.borrow_mut() = state.conditions;
        }
    }

    fn ensure_staged(&self) -> PyResult<()> {
        ensure_staged(&self.applied, &format!("combo '{}'", self.name))
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
impl ComboObject {
    /// Set the key positions that trigger this combo (1-based indices).
    ///
    /// Args:
    ///     keys: List of key positions (1-based).
    ///
    /// Returns:
    ///     Self for method chaining.
    fn keys(&self, keys: &Bound<'_, PyList>) -> PyResult<Self> {
        self.ensure_staged()?;
        let key_positions = pylist_to_u32(keys)?;
        *self.keys.borrow_mut() = Some(key_positions);
        Ok(self.clone())
    }

    /// Set the binding to execute when the combo triggers.
    ///
    /// Args:
    ///     binding: The binding string (e.g., "&kp ESC").
    ///
    /// Returns:
    ///     Self for method chaining.
    fn binding(&self, binding: String) -> PyResult<Self> {
        self.ensure_staged()?;
        *self.binding.borrow_mut() = Some(binding);
        Ok(self.clone())
    }

    /// Set the timeout in milliseconds.
    ///
    /// Args:
    ///     value: Timeout in milliseconds, or None to clear.
    ///
    /// Returns:
    ///     Self for method chaining.
    #[pyo3(signature = (value=None))]
    fn timeout(&self, value: Option<i64>) -> PyResult<Self> {
        self.ensure_staged()?;
        let timeout = pyvalue_to_optional_u32(value)?;
        *self.timeout.borrow_mut() = Some(timeout);
        Ok(self.clone())
    }

    /// Restrict the combo to specific layers.
    ///
    /// Args:
    ///     layers: List of layer names.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn on_layers(&self, layers: &Bound<'_, PyList>) -> PyResult<Self> {
        self.ensure_staged()?;
        let list = pylist_to_strings(layers)?;
        *self.layers.borrow_mut() = list;
        Ok(self.clone())
    }

    /// Add a condition for when this combo is active.
    ///
    /// Args:
    ///     condition: The condition expression.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn when(&self, condition: String) -> PyResult<Self> {
        self.ensure_staged()?;
        self.conditions.borrow_mut().push(condition);
        Ok(self.clone())
    }

    /// Get the current key positions (1-based).
    ///
    /// Returns:
    ///     List of key positions.
    fn get_keys(&self) -> Vec<u32> {
        match self.keys.borrow().as_ref() {
            Some(keys) => keys.iter().map(|k| k + 1).collect(),
            None => Vec::new(),
        }
    }

    /// Get the current binding.
    ///
    /// Returns:
    ///     The binding string or empty string if not set.
    fn get_binding(&self) -> String {
        match &*self.binding.borrow() {
            Some(binding) => binding.clone(),
            None => String::new(),
        }
    }

    /// Get the current timeout.
    ///
    /// Returns:
    ///     The timeout in milliseconds or None.
    fn get_timeout(&self) -> Option<u32> {
        self.timeout.borrow().clone().unwrap_or(None)
    }

    /// Apply the changes to the layout.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn apply(&self) -> PyResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}
