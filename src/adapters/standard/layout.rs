use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use crate::{
    ast::DtItem,
    dts::DtsDocument,
    providers::{
        BehaviorDefinition, BehaviorProvider, ComboProvider, KeymapProvider, ProviderError,
    },
};

use super::{
    listeners::extract_input_listeners,
    types::{BehaviorSpec, ComboSpec, InputListenerSpec, LayerSpec, LayoutMetadata, MacroSpec},
};

/// Adapter-friendly view of the parsed layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterLayout {
    pub layers: Vec<LayerSpec>,
    pub combos: Vec<ComboSpec>,
    pub behaviors: Vec<BehaviorSpec>,
    pub macros: Vec<MacroSpec>,
    pub input_listeners: Vec<InputListenerSpec>,
    pub metadata: LayoutMetadata,
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
                LayerSpec {
                    name,
                    bindings,
                    properties: BTreeMap::new(),
                }
            })
            .collect();
        let combos = ComboProvider::new(document)
            .combos()
            .into_iter()
            .map(ComboSpec::from)
            .collect();
        let mut behaviors = Vec::new();
        let mut macros = Vec::new();
        for definition in BehaviorProvider::new(document).behaviors().into_iter() {
            if behavior_definition_is_macro(&definition) {
                macros.push(MacroSpec::from(definition));
            } else {
                behaviors.push(BehaviorSpec::from(definition));
            }
        }
        let mut layout = Self {
            layers,
            combos,
            behaviors,
            macros,
            input_listeners: extract_input_listeners(document),
            metadata: LayoutMetadata::default(),
        };
        layout.ensure_property_orders();

        if let Some(includes) = extract_header_includes(document) {
            layout
                .metadata
                .extras
                .insert("resolved_includes".into(), Value::String(includes));
        }

        layout
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
            if !layer.properties.is_empty() {
                let props: Vec<(String, String)> = layer
                    .properties
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                provider.set_layer_metadata(&layer.name, &props)?;
            }
        }

        for combo in &self.combos {
            if let Some(binding) = combo.binding.as_deref() {
                provider.upsert_combo(
                    &combo.name,
                    binding,
                    &combo.key_positions,
                    combo.timeout_ms,
                    &combo.layers,
                    &combo.conditions,
                )?;
            }
        }

        for macro_behavior in &self.macros {
            let binding_refs: Vec<&str> = macro_behavior
                .bindings
                .iter()
                .map(|binding| binding.as_str())
                .collect();
            if !binding_refs.is_empty() {
                match provider.set_behavior_bindings(&macro_behavior.name, &binding_refs) {
                    Ok(()) => {}
                    Err(ProviderError::BehaviorNotFound(name)) if name == macro_behavior.name => {
                        // Ignore missing behaviors for macros; they may live under `macros`.
                    }
                    Err(ProviderError::PropertyMissing { .. }) => {
                        // Ignore missing bindings on macros; existing macros keep their bindings.
                    }
                    Err(err) => return Err(err),
                }
            }
            provider.set_macro_timing(
                &macro_behavior.name,
                macro_behavior.wait_ms,
                macro_behavior.tap_ms,
            )?;
            provider
                .set_behavior_binding_cells(&macro_behavior.name, macro_behavior.binding_cells)?;
            provider.set_behavior_label(&macro_behavior.name, macro_behavior.label.as_deref())?;
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

    /// Parse a JSON layout from an already-deserialized value.
    pub fn from_standard_json_value(value: serde_json::Value) -> serde_json::Result<Self> {
        let payload: StandardFormat = serde_json::from_value(value)?;
        Ok(payload.into())
    }

    pub fn ensure_property_orders(&mut self) {
        self.ensure_behavior_property_orders();
        self.ensure_combo_property_orders();
        self.ensure_macro_property_orders();
        self.ensure_input_listener_property_orders();
    }

    fn ensure_behavior_property_orders(&mut self) {
        for behavior in &mut self.behaviors {
            behavior.ensure_property_order();
        }
    }

    fn ensure_combo_property_orders(&mut self) {
        for combo in &mut self.combos {
            combo.ensure_property_order();
        }
    }

    fn ensure_macro_property_orders(&mut self) {
        for mac in &mut self.macros {
            mac.ensure_property_order();
        }
    }

    fn ensure_input_listener_property_orders(&mut self) {
        for listener in &mut self.input_listeners {
            listener.ensure_property_order();
        }
    }
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
    macros: Vec<MacroSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    behaviors: Vec<BehaviorSpec>,
    #[serde(
        default,
        rename = "inputListeners",
        skip_serializing_if = "Vec::is_empty"
    )]
    input_listeners: Vec<InputListenerSpec>,
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
            macros: layout.macros.clone(),
            behaviors: layout.behaviors.clone(),
            input_listeners: layout.input_listeners.clone(),
        }
    }
}

impl From<StandardFormat> for AdapterLayout {
    fn from(value: StandardFormat) -> Self {
        let mut behaviors = Vec::new();
        let mut macros = value.macros;
        for mut behavior in value.behaviors {
            behavior.ensure_property_order();
            if is_macro_behavior_spec(&behavior) {
                macros.push(MacroSpec::from_behavior_spec(&behavior));
            } else {
                behaviors.push(behavior);
            }
        }
        let mut layout = Self {
            layers: value.layers,
            combos: value.combos,
            behaviors,
            macros,
            input_listeners: value.input_listeners,
            metadata: LayoutMetadata {
                title: value.title,
                description: value.description,
                author: value.author,
                version: value.version,
                extras: value.metadata,
            },
        };
        layout.ensure_property_orders();
        layout
    }
}

fn behavior_definition_is_macro(definition: &BehaviorDefinition) -> bool {
    definition
        .compatible
        .as_deref()
        .map(|compat| compat.contains("behavior-macro"))
        .unwrap_or(false)
}

fn is_macro_behavior_spec(behavior: &BehaviorSpec) -> bool {
    behavior
        .compatible
        .as_deref()
        .map(|compat| compat.contains("behavior-macro"))
        .unwrap_or(false)
}

fn extract_header_includes(document: &DtsDocument) -> Option<String> {
    let mut includes = Vec::new();
    collect_include_statements(&document.items, &mut includes);
    if includes.is_empty() {
        None
    } else {
        Some(includes.join("\n"))
    }
}

fn collect_include_statements(items: &[DtItem], includes: &mut Vec<String>) {
    for item in items {
        match item {
            DtItem::Include(include) => {
                let line = include.text.trim();
                if !line.is_empty() {
                    includes.push(line.to_string());
                }
            }
            DtItem::Node(node) => collect_include_statements(&node.children, includes),
            DtItem::Conditional(cond) => {
                for branch in &cond.branches {
                    collect_include_statements(&branch.items, includes);
                }
            }
            _ => {}
        }
    }
}
