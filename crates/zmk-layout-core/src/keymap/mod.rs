use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::{
    standard::{
        BehaviorSpec, ComboSpec, InputListenerSpec, LayerSpec, LayoutMetadata, MacroSpec,
        TemplateParseMode,
    },
    AdapterLayout,
};
use crate::dts::DtsDocument;
use crate::providers::ProviderError;

/// Semantic keymap representation decoupled from Devicetree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeymapDocument {
    pub layers: Vec<LayerSpec>,
    pub combos: Vec<ComboSpec>,
    pub behaviors: Vec<BehaviorSpec>,
    pub macros: Vec<MacroSpec>,
    pub input_listeners: Vec<InputListenerSpec>,
    pub metadata: KeymapMetadata,
}

/// Rich metadata carried alongside the keymap.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeymapMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub extras: BTreeMap<String, Value>,
    #[serde(skip)]
    pub template: Option<TemplateInfo>,
}

/// Template context describing how the keymap was parsed/rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInfo {
    pub source: Option<String>,
    pub mode: TemplateParseMode,
}

impl Default for TemplateInfo {
    fn default() -> Self {
        Self {
            source: None,
            mode: TemplateParseMode::default(),
        }
    }
}

impl From<AdapterLayout> for KeymapDocument {
    fn from(value: AdapterLayout) -> Self {
        let LayoutMetadata {
            title,
            description,
            author,
            version,
            extras,
        } = value.metadata;
        Self {
            layers: value.layers,
            combos: value.combos,
            behaviors: value.behaviors,
            macros: value.macros,
            input_listeners: value.input_listeners,
            metadata: KeymapMetadata {
                title,
                description,
                author,
                version,
                extras,
                template: None,
            },
        }
    }
}

impl From<KeymapDocument> for AdapterLayout {
    fn from(value: KeymapDocument) -> Self {
        AdapterLayout {
            layers: value.layers,
            combos: value.combos,
            behaviors: value.behaviors,
            macros: value.macros,
            input_listeners: value.input_listeners,
            metadata: LayoutMetadata {
                title: value.metadata.title,
                description: value.metadata.description,
                author: value.metadata.author,
                version: value.metadata.version,
                extras: value.metadata.extras,
            },
        }
    }
}

impl KeymapDocument {
    pub fn from_document(document: DtsDocument) -> Self {
        let adapter = AdapterLayout::from_document(&document);
        let mut keymap = KeymapDocument::from(adapter);
        keymap
            .metadata
            .extras
            .entry("original_format".into())
            .or_insert(Value::String("dts".into()));
        keymap
    }

    pub fn parse_str(source: &str) -> Result<Self, crate::tokenizer::LayoutError> {
        let doc = DtsDocument::parse_str(source)?;
        Ok(Self::from_document(doc))
    }

    pub fn layer_names(&self) -> Vec<String> {
        self.layers.iter().map(|layer| layer.name.clone()).collect()
    }

    pub fn bindings_for_layer(&self, layer: &str) -> Result<Vec<String>, ProviderError> {
        let layer = self
            .layers
            .iter()
            .find(|entry| entry.name == layer)
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        Ok(layer.bindings.clone())
    }

    pub fn set_binding(
        &mut self,
        layer: &str,
        index: usize,
        binding: &str,
    ) -> Result<(), ProviderError> {
        let layer = self
            .layers
            .iter_mut()
            .find(|entry| entry.name == layer)
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        if index >= layer.bindings.len() {
            return Err(ProviderError::BindingIndex {
                index,
                len: layer.bindings.len(),
            });
        }
        layer.bindings[index] = binding.to_string();
        Ok(())
    }

    pub fn set_layer_bindings(
        &mut self,
        layer: &str,
        bindings: &[&str],
    ) -> Result<(), ProviderError> {
        let layer = self
            .layers
            .iter_mut()
            .find(|entry| entry.name == layer)
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        if bindings.is_empty() {
            return Err(ProviderError::InvalidBinding(
                "layer must have at least one binding".into(),
            ));
        }
        layer.bindings = bindings.iter().map(|b| b.to_string()).collect();
        Ok(())
    }

    pub fn set_layer_metadata(
        &mut self,
        layer: &str,
        metadata: &[(String, String)],
    ) -> Result<(), ProviderError> {
        let layer = self
            .layers
            .iter_mut()
            .find(|entry| entry.name == layer)
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        for (key, value) in metadata {
            layer.properties.insert(key.clone(), value.clone());
        }
        Ok(())
    }

    pub fn set_behavior_bindings(
        &mut self,
        behavior: &str,
        bindings: &[&str],
    ) -> Result<(), ProviderError> {
        let behavior = self
            .behaviors
            .iter_mut()
            .find(|entry| entry.name == behavior)
            .ok_or_else(|| ProviderError::BehaviorNotFound(behavior.to_string()))?;
        if bindings.is_empty() {
            return Err(ProviderError::InvalidBinding(
                "behavior bindings cannot be empty".into(),
            ));
        }
        behavior.bindings = bindings.iter().map(|b| b.to_string()).collect();
        Ok(())
    }

    pub fn set_behavior_properties(
        &mut self,
        behavior: &str,
        properties: &[(String, String)],
    ) -> Result<(), ProviderError> {
        let behavior = self
            .behaviors
            .iter_mut()
            .find(|entry| entry.name == behavior)
            .ok_or_else(|| ProviderError::BehaviorNotFound(behavior.to_string()))?;
        for (key, value) in properties {
            behavior.properties.insert(key.clone(), value.clone());
        }
        Ok(())
    }

    pub fn set_behavior_binding_cells(
        &mut self,
        behavior: &str,
        binding_cells: Option<u32>,
    ) -> Result<(), ProviderError> {
        let behavior = self
            .behaviors
            .iter_mut()
            .find(|entry| entry.name == behavior)
            .ok_or_else(|| ProviderError::BehaviorNotFound(behavior.to_string()))?;
        behavior.binding_cells = binding_cells;
        Ok(())
    }

    pub fn set_behavior_label(
        &mut self,
        behavior: &str,
        label: Option<&str>,
    ) -> Result<(), ProviderError> {
        let behavior = self
            .behaviors
            .iter_mut()
            .find(|entry| entry.name == behavior)
            .ok_or_else(|| ProviderError::BehaviorNotFound(behavior.to_string()))?;
        behavior.label = label.map(|v| v.to_string());
        Ok(())
    }

    pub fn upsert_combo(
        &mut self,
        name: &str,
        binding: &str,
        key_positions: &[u32],
        timeout_ms: Option<u32>,
        layers: &[u32],
        conditions: &[String],
    ) -> Result<(), ProviderError> {
        if key_positions.is_empty() {
            return Err(ProviderError::InvalidBinding(
                "combo requires at least one key position".into(),
            ));
        }
        let binding_str = binding.to_string();
        if let Some(existing) = self.combos.iter_mut().find(|combo| combo.name == name) {
            existing.binding = Some(binding_str);
            existing.key_positions = key_positions.to_vec();
            existing.timeout_ms = timeout_ms;
            existing.layers = layers.to_vec();
            existing.conditions = conditions.to_vec();
        } else {
            self.combos.push(ComboSpec {
                name: name.to_string(),
                description: String::new(),
                key_positions: key_positions.to_vec(),
                timeout_ms,
                layers: layers.to_vec(),
                binding: Some(binding_str),
                behavior: None,
                properties: BTreeMap::new(),
                conditions: conditions.to_vec(),
                property_order: Vec::new(),
            });
        }
        Ok(())
    }

    pub fn reorder_layer(&mut self, layer: &str, position: usize) -> Result<(), ProviderError> {
        let current = self
            .layers
            .iter()
            .position(|entry| entry.name == layer)
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        let len_before = self.layers.len();
        if position > len_before {
            return Err(ProviderError::LayerNotFound(layer.to_string()));
        }
        let entry = self.layers.remove(current);
        let mut target = position;
        if position > current {
            target = target.saturating_sub(1);
        }
        let target = target.min(self.layers.len());
        self.layers.insert(target, entry);
        Ok(())
    }

    pub fn add_layer(&mut self, name: &str, bindings: &[&str]) -> Result<(), ProviderError> {
        if bindings.is_empty() {
            return Err(ProviderError::InvalidBinding(
                "layer must have at least one binding".into(),
            ));
        }
        if self.layers.iter().any(|layer| layer.name == name) {
            return Err(ProviderError::LayerNotFound(name.to_string()));
        }
        self.layers.push(LayerSpec {
            name: name.to_string(),
            bindings: bindings.iter().map(|b| b.to_string()).collect(),
            properties: BTreeMap::new(),
        });
        Ok(())
    }

    pub fn remove_layer(&mut self, name: &str) -> Result<(), ProviderError> {
        if let Some(idx) = self.layers.iter().position(|layer| layer.name == name) {
            self.layers.remove(idx);
            return Ok(());
        }
        Err(ProviderError::LayerNotFound(name.to_string()))
    }
}
