use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::{
    AdapterLayout,
    standard::{
        BehaviorSpec, ComboSpec, InputListenerSpec, LayerSpec, LayoutMetadata, MacroSpec,
        TemplateParseMode,
    },
};

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
