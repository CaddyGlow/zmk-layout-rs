use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use toml::Value as TomlValue;

use zmk_layout_core::adapters::BehaviorSpec;

use super::util::{SharedLayout, ensure_staged, pylist_to_strings, script_error};

/// Builder for configuring behaviors.
///
/// Use the fluent API to configure the behavior, then call apply() to commit changes.
#[pyclass(unsendable)]
#[derive(Clone)]
pub struct BehaviorObject {
    name: String,
    layout: SharedLayout,
    params: RefCell<BTreeMap<String, TomlValue>>,
    bindings: RefCell<Option<Vec<String>>>,
    applied: Cell<bool>,
}

impl BehaviorObject {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        let behavior = Self {
            name,
            layout,
            params: RefCell::new(BTreeMap::new()),
            bindings: RefCell::new(None),
            applied: Cell::new(false),
        };
        behavior.seed_from_layout();
        behavior
    }

    pub fn as_binding_string(&self) -> PyResult<String> {
        self.ensure_applied()?;
        Ok(format!("&{}", self.name))
    }

    fn apply_internal(&self) -> PyResult<()> {
        self.ensure_staged()?;
        let mut metadata = self.params.borrow().clone();
        if let Some(bindings) = self.bindings.borrow().clone() {
            let boxed = bindings
                .into_iter()
                .map(TomlValue::String)
                .collect::<Vec<_>>();
            metadata.insert("bindings".to_string(), TomlValue::Array(boxed));
        }
        let mut engine = self.layout.borrow_mut();
        engine
            .set_behavior_settings(&self.name, &metadata)
            .map_err(|err| script_error(err.to_string()))?;
        self.applied.set(true);
        Ok(())
    }

    fn behavior_definition(&self) -> Option<BehaviorSpec> {
        let engine = self.layout.borrow();
        engine
            .document()
            .behaviors
            .iter()
            .find(|behavior| behavior.name == self.name)
            .cloned()
    }

    fn seed_from_layout(&self) {
        if let Some(def) = self.behavior_definition() {
            if !def.bindings.is_empty() {
                self.bindings.borrow_mut().replace(def.bindings);
            }
        }
    }

    fn ensure_staged(&self) -> PyResult<()> {
        ensure_staged(&self.applied, &format!("behavior '{}'", self.name))
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
impl BehaviorObject {
    /// Set a parameter on the behavior.
    ///
    /// Args:
    ///     key: The parameter name.
    ///     value: The parameter value (string, int, float, bool, list, or dict).
    ///
    /// Returns:
    ///     Self for method chaining.
    fn param(&self, key: String, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        self.ensure_staged()?;
        let toml = pyany_to_toml(value)?;
        self.params.borrow_mut().insert(key, toml);
        Ok(self.clone())
    }

    /// Set the bindings for this behavior.
    ///
    /// Args:
    ///     bindings: List of binding strings.
    ///
    /// Returns:
    ///     Self for method chaining.
    fn bindings(&self, bindings: &Bound<'_, PyList>) -> PyResult<Self> {
        self.ensure_staged()?;
        let list = pylist_to_strings(bindings)?;
        self.bindings.borrow_mut().replace(list);
        Ok(self.clone())
    }

    /// Get a parameter value.
    ///
    /// Args:
    ///     key: The parameter name.
    ///
    /// Returns:
    ///     The parameter value or None if not set.
    fn get_param(&self, py: Python<'_>, key: String) -> PyResult<Py<PyAny>> {
        if let Some(value) = self.params.borrow().get(&key) {
            return toml_to_pyobject(py, value);
        }
        if let Some(def) = self.behavior_definition() {
            if let Some(value) = def.properties.get(&key) {
                return Ok(value.clone().into_pyobject(py)?.unbind().into());
            }
        }
        Ok(py.None().into())
    }

    /// Get the current bindings.
    ///
    /// Returns:
    ///     List of binding strings.
    fn get_bindings(&self) -> Vec<String> {
        if let Some(bindings) = self.bindings.borrow().as_ref() {
            return bindings.clone();
        }
        if let Some(def) = self.behavior_definition() {
            return def.bindings.clone();
        }
        Vec::new()
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

fn pyany_to_toml(value: &Bound<'_, PyAny>) -> PyResult<TomlValue> {
    if value.is_none() {
        return Err(script_error("parameter values cannot be None"));
    }
    if let Ok(flag) = value.extract::<bool>() {
        return Ok(TomlValue::Boolean(flag));
    }
    if let Ok(num) = value.extract::<i64>() {
        return Ok(TomlValue::Integer(num));
    }
    if let Ok(num) = value.extract::<f64>() {
        return Ok(TomlValue::Float(num));
    }
    if let Ok(text) = value.extract::<String>() {
        return Ok(TomlValue::String(text));
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut items = Vec::new();
        for item in list.iter() {
            items.push(pyany_to_toml(&item)?);
        }
        return Ok(TomlValue::Array(items));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut map = toml::map::Map::new();
        for (key, val) in dict.iter() {
            let key: String = key.extract()?;
            map.insert(key, pyany_to_toml(&val)?);
        }
        return Ok(TomlValue::Table(map));
    }
    Err(script_error(format!(
        "unsupported value type {} for parameter",
        value.get_type().name()?
    )))
}

fn toml_to_pyobject(py: Python<'_>, value: &TomlValue) -> PyResult<Py<PyAny>> {
    Ok(match value {
        TomlValue::String(text) => text.clone().into_pyobject(py)?.unbind().into_any(),
        TomlValue::Integer(num) => (*num).into_pyobject(py)?.unbind().into_any(),
        TomlValue::Float(num) => (*num).into_pyobject(py)?.unbind().into_any(),
        TomlValue::Boolean(flag) => (*flag).into_pyobject(py)?.to_owned().unbind().into_any(),
        TomlValue::Array(items) => {
            let list = PyList::empty(py);
            for entry in items {
                list.append(toml_to_pyobject(py, entry)?)?;
            }
            list.unbind().into_any()
        }
        TomlValue::Table(entries) => {
            let dict = PyDict::new(py);
            for (key, entry) in entries {
                dict.set_item(key, toml_to_pyobject(py, entry)?)?;
            }
            dict.unbind().into_any()
        }
        TomlValue::Datetime(dt) => dt.to_string().into_pyobject(py)?.unbind().into_any(),
    })
}
