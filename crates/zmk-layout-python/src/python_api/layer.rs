use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::PyList;

use super::{
    behavior::BehaviorObject,
    combo::ComboObject,
    macro_builder::MacroObject,
    util::{SharedLayout, ensure_staged, require_positive_index, script_error},
};

/// Builder for modifying layer bindings.
///
/// Use the fluent API to configure the layer, then call apply() to commit changes.
#[pyclass(unsendable)]
#[derive(Clone)]
pub struct LayerBuilder {
    name: String,
    layout: SharedLayout,
    staged: RefCell<HashMap<usize, String>>,
    replace_all: RefCell<Option<Vec<String>>>,
    metadata: RefCell<HashMap<String, String>>,
    applied: Cell<bool>,
}

impl LayerBuilder {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        Self {
            name,
            layout,
            staged: RefCell::new(HashMap::new()),
            replace_all: RefCell::new(None),
            metadata: RefCell::new(HashMap::new()),
            applied: Cell::new(false),
        }
    }

    fn ensure_staged(&self) -> PyResult<()> {
        ensure_staged(&self.applied, &format!("layer '{}'", self.name))
    }

    fn coerce_binding(&self, value: &Bound<'_, PyAny>) -> PyResult<String> {
        if let Ok(s) = value.extract::<String>() {
            return Ok(s);
        }
        if let Ok(combo) = value.extract::<ComboObject>() {
            return combo.as_binding_string();
        }
        if let Ok(mac) = value.extract::<MacroObject>() {
            return mac.as_binding_string();
        }
        if let Ok(behavior) = value.extract::<BehaviorObject>() {
            return behavior.as_binding_string();
        }
        Err(script_error(format!(
            "binding must be string or object, got {}",
            value.get_type().name()?
        )))
    }

    fn apply_internal(&self) -> PyResult<()> {
        self.ensure_staged()?;

        let mut engine = self.layout.borrow_mut();

        // Determine final bindings
        let (bindings, layer_exists) = if let Some(all) = self.replace_all.borrow_mut().take() {
            let normalized = engine
                .normalize_binding_list(&all)
                .map_err(|err| script_error(err.to_string()))?;
            let exists = engine.layer_names().contains(&self.name);
            (normalized, exists)
        } else {
            let mut current = engine
                .layer_bindings(&self.name)
                .map_err(|err| script_error(err.to_string()))?;
            for (index, binding) in self.staged.borrow().iter() {
                if *index >= current.len() {
                    return Err(script_error(format!(
                        "binding index {} out of range for layer '{}'",
                        index + 1,
                        self.name
                    )));
                }
                current[*index] = binding.clone();
            }
            let normalized = engine
                .normalize_binding_list(&current)
                .map_err(|err| script_error(err.to_string()))?;
            (normalized, true)
        };

        if layer_exists {
            engine
                .set_layer_bindings(&self.name, &bindings)
                .map_err(|err| script_error(err.to_string()))?;
        } else {
            engine
                .add_layer(&self.name, &bindings)
                .map_err(|err| script_error(err.to_string()))?;
        }

        if !self.metadata.borrow().is_empty() {
            let props: Vec<(String, String)> = self
                .metadata
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            engine
                .set_layer_metadata(&self.name, &props)
                .map_err(|err| script_error(err.to_string()))?;
        }

        self.applied.set(true);
        Ok(())
    }
}

#[pymethods]
impl LayerBuilder {
    /// Set a single binding at a specific index (1-based).
    ///
    /// Args:
    ///     index: The position to set (1-based).
    ///     binding: The binding string or object.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn bind(&self, index: i64, binding: &Bound<'_, PyAny>) -> PyResult<Self> {
        self.ensure_staged()?;
        let normalized_index = require_positive_index(index, "binding")?;
        let binding_text = self.coerce_binding(binding)?;
        self.replace_all.borrow_mut().take(); // switch to partial mode
        self.staged
            .borrow_mut()
            .insert(normalized_index, binding_text);
        Ok(self.clone())
    }

    /// Replace all bindings with a new list.
    ///
    /// Args:
    ///     bindings: List of binding strings or objects.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn bindings(&self, bindings: &Bound<'_, PyList>) -> PyResult<Self> {
        self.ensure_staged()?;
        let mut result = Vec::new();
        for item in bindings.iter() {
            let binding_text = self.coerce_binding(&item)?;
            result.push(binding_text);
        }
        self.replace_all.borrow_mut().replace(result);
        self.staged.borrow_mut().clear();
        Ok(self.clone())
    }

    /// Set layer metadata.
    ///
    /// Args:
    ///     key: The metadata key.
    ///     value: The metadata value.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn meta(&self, key: String, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        self.ensure_staged()?;
        let rendered = if let Ok(flag) = value.extract::<bool>() {
            flag.to_string()
        } else if let Ok(num) = value.extract::<i64>() {
            num.to_string()
        } else if let Ok(num) = value.extract::<f64>() {
            num.to_string()
        } else if let Ok(s) = value.extract::<String>() {
            s
        } else if value.is_none() {
            return Err(script_error("metadata values cannot be None"));
        } else {
            return Err(script_error(format!(
                "unsupported metadata type {}",
                value.get_type().name()?
            )));
        };
        self.metadata.borrow_mut().insert(key, rendered);
        Ok(self.clone())
    }

    /// Get a binding at a specific index (1-based).
    ///
    /// Args:
    ///     index: The position to get (1-based).
    ///
    /// Returns:
    ///     The binding string at that position.
    fn get_binding(&self, index: i64) -> PyResult<String> {
        let normalized_index = require_positive_index(index, "binding")?;
        let engine = self.layout.borrow();
        let layer = engine
            .get_layer(&self.name)
            .ok_or_else(|| script_error(format!("layer '{}' not found", self.name)))?;
        let value = layer
            .bindings
            .get(normalized_index)
            .ok_or_else(|| script_error("binding index out of range"))?;
        Ok(value.clone())
    }

    /// Get all bindings for this layer.
    ///
    /// Returns:
    ///     List of all binding strings.
    fn get_bindings(&self) -> PyResult<Vec<String>> {
        let engine = self.layout.borrow();
        let layer = engine
            .get_layer(&self.name)
            .ok_or_else(|| script_error(format!("layer '{}' not found", self.name)))?;
        Ok(layer.bindings.clone())
    }

    /// Get a metadata value.
    ///
    /// Args:
    ///     key: The metadata key.
    ///
    /// Returns:
    ///     The metadata value as a string.
    fn get_meta(&self, key: String) -> String {
        let engine = self.layout.borrow();
        engine
            .meta_to_string(&key)
            .unwrap_or_else(|| String::new())
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
