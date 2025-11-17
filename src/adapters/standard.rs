//! Simplified adapter that exposes combo/behavior metadata for external tooling.

use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use crate::{
    dts::{DtsDocument, DtsError},
    providers::{
        BehaviorDefinition, BehaviorProvider, ComboDefinition, ComboProvider, KeymapProvider,
        ProviderError,
    },
};
use thiserror::Error;

/// Errors surfaced by the adapter helpers.
#[derive(Debug, Error)]
pub enum AdapterError {
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Dts(#[from] DtsError),
}

/// Minimal combo representation consumable by adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComboSpec {
    pub name: String,
    pub key_positions: Vec<u32>,
    pub timeout_ms: Option<u32>,
    pub bindings: Vec<String>,
}

impl From<ComboDefinition> for ComboSpec {
    fn from(value: ComboDefinition) -> Self {
        Self {
            name: value.name,
            key_positions: value.key_positions,
            timeout_ms: value.timeout_ms,
            bindings: value
                .bindings
                .into_iter()
                .map(|binding| binding.to_binding_string())
                .collect(),
        }
    }
}

/// Minimal behavior representation consumable by adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehaviorSpec {
    pub name: String,
    pub compatible: Option<String>,
    pub binding_cells: Option<u32>,
    pub bindings: Vec<String>,
}

impl From<BehaviorDefinition> for BehaviorSpec {
    fn from(value: BehaviorDefinition) -> Self {
        Self {
            name: value.name,
            compatible: value.compatible,
            binding_cells: value.binding_cells,
            bindings: value.bindings,
        }
    }
}

/// Adapter-friendly view of the parsed layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdapterLayout {
    pub layers: Vec<LayerSpec>,
    pub combos: Vec<ComboSpec>,
    pub behaviors: Vec<BehaviorSpec>,
    pub metadata: LayoutMetadata,
}

/// Layer description containing its name and bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerSpec {
    pub name: String,
    pub bindings: Vec<String>,
}

/// Metadata describing the layout when exported to other formats.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub extras: BTreeMap<String, Value>,
}

impl AdapterLayout {
    /// Extract adapter-specific data from a [`DtsDocument`].
    pub fn from_document(document: &DtsDocument) -> Self {
        let layer_provider = KeymapProvider::new(document.clone());
        let layers = layer_provider
            .layer_names()
            .into_iter()
            .map(|name| {
                let bindings = layer_provider
                    .bindings_for_layer(&name)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|binding| binding.to_binding_string())
                    .collect();
                LayerSpec { name, bindings }
            })
            .collect();
        let combos = ComboProvider::new(document)
            .combos()
            .into_iter()
            .map(ComboSpec::from)
            .collect();
        let behaviors = BehaviorProvider::new(document)
            .behaviors()
            .into_iter()
            .map(BehaviorSpec::from)
            .collect();
        Self {
            layers,
            combos,
            behaviors,
            metadata: LayoutMetadata::default(),
        }
    }

    /// Convenience helper that extracts data using a [`KeymapProvider`].
    pub fn from_provider(provider: &KeymapProvider) -> Self {
        Self::from_document(provider.document())
    }

    /// Apply the adapter data back onto the Devicetree document via the provider APIs.
    pub fn apply_to_provider(&self, provider: &mut KeymapProvider) -> Result<(), ProviderError> {
        for layer in &self.layers {
            let bindings: Vec<&str> = layer
                .bindings
                .iter()
                .map(|binding| binding.as_str())
                .collect();
            provider.set_layer_bindings(&layer.name, &bindings)?;
        }

        for combo in &self.combos {
            provider.set_combo_key_positions(&combo.name, &combo.key_positions)?;
            provider.set_combo_timeout_ms(&combo.name, combo.timeout_ms)?;
            let binding_refs: Vec<&str> = combo.bindings.iter().map(|b| b.as_str()).collect();
            provider.set_combo_bindings(&combo.name, &binding_refs)?;
        }

        for behavior in &self.behaviors {
            let binding_refs: Vec<&str> = behavior.bindings.iter().map(|b| b.as_str()).collect();
            if binding_refs.is_empty() {
                return Err(ProviderError::InvalidBinding(format!(
                    "behavior `{}` must contain at least one binding",
                    behavior.name
                )));
            }
            match provider.set_behavior_bindings(&behavior.name, &binding_refs) {
                Ok(()) => {}
                Err(ProviderError::BehaviorNotFound(name)) if name == behavior.name => continue,
                Err(err) => return Err(err),
            }
        }

        Ok(())
    }

    /// Apply the adapter data, returning an updated [`DtsDocument`].
    pub fn apply_to_document(&self, document: DtsDocument) -> Result<DtsDocument, ProviderError> {
        let mut provider = KeymapProvider::new(document);
        self.apply_to_provider(&mut provider)?;
        Ok(provider.into_document())
    }

    /// Serialize the adapter layout into the JSON structure shared with the standard format.
    pub fn to_standard_json(&self) -> serde_json::Result<String> {
        let payload = StandardFormat::from(self);
        serde_json::to_string_pretty(&payload)
    }

    /// Parse a JSON layout produced by [`to_standard_json`].
    pub fn from_standard_json(json: &str) -> serde_json::Result<Self> {
        let payload: StandardFormat = serde_json::from_str(json)?;
        Ok(payload.into())
    }
}

/// Export a document to the standard JSON format.
pub fn export_standard_str(document: &DtsDocument) -> Result<String, AdapterError> {
    Ok(AdapterLayout::from_document(document).to_standard_json()?)
}

/// Export a document directly to a file containing the standard JSON format.
pub fn export_standard_file(
    document: &DtsDocument,
    path: impl AsRef<Path>,
) -> Result<(), AdapterError> {
    let json = export_standard_str(document)?;
    fs::write(path, json)?;
    Ok(())
}

/// Apply a standard JSON payload to a document template.
pub fn import_standard_str(
    json: &str,
    base_document: DtsDocument,
) -> Result<DtsDocument, AdapterError> {
    let layout = AdapterLayout::from_standard_json(json)?;
    Ok(layout.apply_to_document(base_document)?)
}

/// Read a standard JSON file and apply it to the provided document.
pub fn import_standard_file(
    path: impl AsRef<Path>,
    base_document: DtsDocument,
) -> Result<DtsDocument, AdapterError> {
    let text = fs::read_to_string(path)?;
    import_standard_str(&text, base_document)
}

#[derive(Debug, Serialize, Deserialize)]
struct StandardFormat {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    metadata: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layers: Vec<LayerSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    combos: Vec<ComboSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    behaviors: Vec<BehaviorSpec>,
}

impl From<&AdapterLayout> for StandardFormat {
    fn from(layout: &AdapterLayout) -> Self {
        Self {
            title: layout.metadata.title.clone(),
            description: layout.metadata.description.clone(),
            author: layout.metadata.author.clone(),
            version: layout.metadata.version.clone(),
            metadata: layout.metadata.extras.clone(),
            layers: layout.layers.clone(),
            combos: layout.combos.clone(),
            behaviors: layout.behaviors.clone(),
        }
    }
}

impl From<StandardFormat> for AdapterLayout {
    fn from(value: StandardFormat) -> Self {
        Self {
            layers: value.layers,
            combos: value.combos,
            behaviors: value.behaviors,
            metadata: LayoutMetadata {
                title: value.title,
                description: value.description,
                author: value.author,
                version: value.version,
                extras: value.metadata,
            },
        }
    }
}
