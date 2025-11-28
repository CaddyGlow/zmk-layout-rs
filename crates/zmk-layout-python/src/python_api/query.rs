use std::collections::BTreeMap;

use pyo3::prelude::*;

use zmk_layout_core::adapters::{BehaviorSpec, ComboSpec};
use zmk_layout_core::layout_engine::LayoutEngine;

/// Read-only information about a layer.
#[pyclass]
#[derive(Clone)]
pub struct LayerInfo {
    #[pyo3(get)]
    name: String,
    bindings: Vec<String>,
    metadata: BTreeMap<String, String>,
}

impl LayerInfo {
    pub fn from_engine(engine: &LayoutEngine, name: &str) -> PyResult<Option<Self>> {
        if let Some(info) = engine.get_layer(name) {
            Ok(Some(Self {
                name: info.name,
                bindings: info.bindings,
                metadata: BTreeMap::new(),
            }))
        } else {
            Ok(None)
        }
    }
}

#[pymethods]
impl LayerInfo {
    /// Get all bindings for this layer.
    ///
    /// Returns:
    ///     List of binding strings.
    fn bindings(&self) -> Vec<String> {
        self.bindings.clone()
    }

    /// Get the metadata for this layer.
    ///
    /// Returns:
    ///     Dictionary of metadata key-value pairs.
    fn metadata(&self) -> BTreeMap<String, String> {
        self.metadata.clone()
    }

    fn __repr__(&self) -> String {
        format!("LayerInfo(name='{}', bindings={})", self.name, self.bindings.len())
    }
}

/// Read-only information about a combo.
#[pyclass]
#[derive(Clone)]
pub struct ComboInfo {
    #[pyo3(get)]
    name: String,
    keys: Vec<u32>,
    binding: Option<String>,
    timeout: Option<u32>,
}

impl ComboInfo {
    pub fn from_definition(def: ComboSpec) -> Self {
        let binding = def.binding.clone();
        Self {
            name: def.name,
            keys: def.key_positions,
            binding,
            timeout: def.timeout_ms,
        }
    }
}

#[pymethods]
impl ComboInfo {
    /// Get the key positions (1-based indices).
    ///
    /// Returns:
    ///     List of key positions.
    fn keys(&self) -> Vec<u32> {
        self.keys.iter().map(|k| k + 1).collect()
    }

    /// Get the binding for this combo.
    ///
    /// Returns:
    ///     The binding string or None.
    fn binding(&self) -> Option<String> {
        self.binding.clone()
    }

    /// Get the timeout in milliseconds.
    ///
    /// Returns:
    ///     The timeout value or None.
    fn timeout(&self) -> Option<u32> {
        self.timeout
    }

    fn __repr__(&self) -> String {
        format!(
            "ComboInfo(name='{}', keys={:?}, binding={:?})",
            self.name,
            self.keys().iter().collect::<Vec<_>>(),
            self.binding
        )
    }
}

/// Read-only information about a behavior.
#[pyclass]
#[derive(Clone)]
pub struct BehaviorInfo {
    #[pyo3(get)]
    name: String,
    bindings: Vec<String>,
    properties: BTreeMap<String, String>,
}

impl BehaviorInfo {
    pub fn from_definition(def: BehaviorSpec) -> Self {
        let properties = def.properties.clone();
        Self {
            name: def.name,
            bindings: def.bindings,
            properties,
        }
    }
}

#[pymethods]
impl BehaviorInfo {
    /// Get all bindings for this behavior.
    ///
    /// Returns:
    ///     List of binding strings.
    fn bindings(&self) -> Vec<String> {
        self.bindings.clone()
    }

    /// Get a property value by key.
    ///
    /// Args:
    ///     key: The property name.
    ///
    /// Returns:
    ///     The property value or None.
    fn get(&self, key: String) -> Option<String> {
        self.properties.get(&key).cloned()
    }

    fn __repr__(&self) -> String {
        format!(
            "BehaviorInfo(name='{}', bindings={})",
            self.name,
            self.bindings.len()
        )
    }
}

pub fn list_combo_definitions(engine: &LayoutEngine) -> Vec<ComboSpec> {
    engine.document().combos.clone()
}

pub fn list_behavior_definitions(engine: &LayoutEngine) -> Vec<BehaviorSpec> {
    engine.document().behaviors.clone()
}
