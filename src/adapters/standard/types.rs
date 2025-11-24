use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, de::Deserializer, ser::Serializer};
use serde_json::Value;

use crate::providers::{BehaviorDefinition, ComboDefinition};

/// Minimal combo representation consumable by adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComboSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(alias = "key_positions")]
    pub key_positions: Vec<u32>,
    #[serde(alias = "timeout_ms")]
    pub timeout_ms: Option<u32>,
    #[serde(default, alias = "layers")]
    pub layers: Vec<u32>,
    #[serde(
        default,
        rename = "binding",
        alias = "bindings",
        deserialize_with = "deserialize_combo_binding",
        serialize_with = "serialize_combo_binding",
        skip_serializing_if = "Option::is_none"
    )]
    pub binding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,
    #[serde(
        default,
        rename = "propertyOrder",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub property_order: Vec<String>,
}

impl From<ComboDefinition> for ComboSpec {
    fn from(value: ComboDefinition) -> Self {
        let binding = value
            .bindings
            .into_iter()
            .map(|binding| binding.to_binding_string())
            .next();
        let mut extras = BTreeMap::new();
        let mut property_order = Vec::new();
        for prop in value.properties {
            property_order.push(prop.name.clone());
            if matches!(
                prop.name.as_str(),
                "timeout-ms" | "key-positions" | "bindings" | "layers" | "description"
            ) {
                continue;
            }
            extras.insert(prop.name, prop.raw_value.unwrap_or_default());
        }
        Self {
            name: value.name,
            key_positions: value.key_positions,
            timeout_ms: value.timeout_ms,
            layers: value.layers,
            description: value.description.unwrap_or_default(),
            binding,
            behavior: None,
            properties: extras,
            conditions: value.conditions,
            property_order,
        }
    }
}

impl ComboSpec {
    pub(crate) fn property_value(&self, name: &str) -> Option<&str> {
        self.properties.get(name).map(|value| value.as_str())
    }

    pub(crate) fn ensure_property_order(&mut self) {
        if self.property_order.is_empty() {
            self.property_order = self.default_property_order();
        }
    }

    pub(crate) fn default_property_order(&self) -> Vec<String> {
        let mut order = Vec::new();
        if self.timeout_ms.is_some() {
            order.push("timeout-ms".to_string());
        }
        if !self.key_positions.is_empty() {
            order.push("key-positions".to_string());
        }
        if self.binding.is_some() {
            order.push("bindings".to_string());
        }
        if !self.layers.is_empty() {
            order.push("layers".to_string());
        }
        for key in self.properties.keys() {
            order.push(key.clone());
        }
        order
    }

    pub(crate) fn resolved_property_order(&self) -> Vec<String> {
        if self.property_order.is_empty() {
            self.default_property_order()
        } else {
            self.property_order.clone()
        }
    }
}

/// Minimal behavior representation consumable by adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehaviorSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub compatible: Option<String>,
    pub binding_cells: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub bindings: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    #[serde(
        default,
        rename = "propertyOrder",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub property_order: Vec<String>,
}

impl From<BehaviorDefinition> for BehaviorSpec {
    fn from(value: BehaviorDefinition) -> Self {
        let BehaviorDefinition {
            name,
            compatible,
            binding_cells,
            bindings,
            description,
            wait_ms: _,
            tap_ms: _,
            label,
            properties,
        } = value;
        let mut extras = BTreeMap::new();
        let mut property_order = Vec::new();
        for prop in properties {
            property_order.push(prop.name.clone());
            if matches!(
                prop.name.as_str(),
                "compatible" | "#binding-cells" | "bindings" | "label"
            ) {
                continue;
            }
            extras.insert(prop.name, prop.raw_value.unwrap_or_default());
        }
        Self {
            name,
            description: description.unwrap_or_default(),
            compatible,
            binding_cells,
            label,
            bindings,
            properties: extras,
            property_order,
        }
    }
}

impl BehaviorSpec {
    pub(crate) fn property_value(&self, name: &str) -> Option<&str> {
        self.properties.get(name).map(|value| value.as_str())
    }

    pub(crate) fn ensure_property_order(&mut self) {
        if self.property_order.is_empty() {
            self.property_order = self.default_property_order();
        }
    }

    pub(crate) fn default_property_order(&self) -> Vec<String> {
        let mut order = Vec::new();
        if self.label.is_some() {
            order.push("label".to_string());
        }
        if self.compatible.is_some() {
            order.push("compatible".to_string());
        }
        if self.binding_cells.is_some() {
            order.push("#binding-cells".to_string());
        }
        if !self.bindings.is_empty() {
            order.push("bindings".to_string());
        }
        for key in self.properties.keys() {
            order.push(key.clone());
        }
        order
    }

    pub(crate) fn resolved_property_order(&self) -> Vec<String> {
        if self.property_order.is_empty() {
            self.default_property_order()
        } else {
            self.property_order.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(alias = "wait_ms")]
    pub wait_ms: Option<u32>,
    #[serde(alias = "tap_ms")]
    pub tap_ms: Option<u32>,
    #[serde(default)]
    pub bindings: Vec<String>,
    #[serde(alias = "binding_cells")]
    pub binding_cells: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatible: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    #[serde(
        default,
        rename = "propertyOrder",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub property_order: Vec<String>,
}

impl From<BehaviorDefinition> for MacroSpec {
    fn from(value: BehaviorDefinition) -> Self {
        let mut extras = BTreeMap::new();
        let mut property_order = Vec::new();
        for prop in value.properties {
            property_order.push(prop.name.clone());
            if matches!(
                prop.name.as_str(),
                "label" | "compatible" | "#binding-cells" | "bindings" | "tap-ms" | "wait-ms"
            ) {
                continue;
            }
            extras.insert(prop.name, prop.raw_value.unwrap_or_default());
        }
        Self {
            name: value.name,
            description: value.description.unwrap_or_default(),
            wait_ms: value.wait_ms,
            tap_ms: value.tap_ms,
            bindings: value.bindings,
            binding_cells: value.binding_cells,
            compatible: value.compatible,
            label: value.label,
            properties: extras,
            property_order,
        }
    }
}

impl MacroSpec {
    pub(crate) fn from_behavior_spec(spec: &BehaviorSpec) -> Self {
        let wait_ms = spec
            .property_value("wait-ms")
            .and_then(|raw| parse_numeric_property(raw));
        let tap_ms = spec
            .property_value("tap-ms")
            .and_then(|raw| parse_numeric_property(raw));
        Self {
            name: spec.name.clone(),
            description: spec.description.clone(),
            wait_ms,
            tap_ms,
            bindings: spec.bindings.clone(),
            binding_cells: spec.binding_cells,
            compatible: spec.compatible.clone(),
            label: spec.label.clone(),
            properties: spec.properties.clone(),
            property_order: spec.property_order.clone(),
        }
    }

    pub(crate) fn property_value(&self, name: &str) -> Option<&str> {
        self.properties.get(name).map(|value| value.as_str())
    }

    pub(crate) fn ensure_property_order(&mut self) {
        if self.property_order.is_empty() {
            self.property_order = self.default_property_order();
        }
    }

    pub(crate) fn default_property_order(&self) -> Vec<String> {
        let mut order = Vec::new();
        order.push("compatible".to_string());
        order.push("#binding-cells".to_string());
        if self.label.is_some() {
            order.insert(0, "label".to_string());
        }
        if self.tap_ms.is_some() {
            order.push("tap-ms".to_string());
        }
        if self.wait_ms.is_some() {
            order.push("wait-ms".to_string());
        }
        if !self.bindings.is_empty() {
            order.push("bindings".to_string());
        }
        for key in self.properties.keys() {
            order.push(key.clone());
        }
        order
    }

    pub(crate) fn resolved_property_order(&self) -> Vec<String> {
        if self.property_order.is_empty() {
            self.default_property_order()
        } else {
            self.property_order.clone()
        }
    }
}

/// Minimal representation of an input processor and its parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputProcessorSpec {
    pub code: String,
    #[serde(default)]
    pub params: Vec<Value>,
}

/// Node entry associated with an input listener.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputListenerNodeSpec {
    pub code: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub layers: Vec<u32>,
    #[serde(default)]
    pub input_processors: Vec<InputProcessorSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    #[serde(
        default,
        rename = "propertyOrder",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub property_order: Vec<String>,
}

/// Representation of an input listener block attached to the keymap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputListenerSpec {
    pub code: String,
    #[serde(default)]
    pub input_processors: Vec<InputProcessorSpec>,
    #[serde(default)]
    pub nodes: Vec<InputListenerNodeSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    #[serde(
        default,
        rename = "propertyOrder",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub property_order: Vec<String>,
}

impl InputListenerSpec {
    pub(crate) fn ensure_property_order(&mut self) {
        if self.property_order.is_empty() {
            self.property_order = self.default_property_order();
        }
        for node in &mut self.nodes {
            node.ensure_property_order();
        }
    }

    pub(crate) fn default_property_order(&self) -> Vec<String> {
        let mut order = Vec::new();
        if !self.input_processors.is_empty() {
            order.push("input-processors".to_string());
        }
        for key in self.properties.keys() {
            order.push(key.clone());
        }
        order
    }

    pub(crate) fn resolved_property_order(&self) -> Vec<String> {
        if self.property_order.is_empty() {
            self.default_property_order()
        } else {
            self.property_order.clone()
        }
    }
}

impl InputListenerNodeSpec {
    pub(crate) fn ensure_property_order(&mut self) {
        if self.property_order.is_empty() {
            self.property_order = self.default_property_order();
        }
    }

    pub(crate) fn default_property_order(&self) -> Vec<String> {
        let mut order = Vec::new();
        if !self.layers.is_empty() {
            order.push("layers".to_string());
        }
        if !self.input_processors.is_empty() {
            order.push("input-processors".to_string());
        }
        for key in self.properties.keys() {
            order.push(key.clone());
        }
        order
    }

    pub(crate) fn resolved_property_order(&self) -> Vec<String> {
        if self.property_order.is_empty() {
            self.default_property_order()
        } else {
            self.property_order.clone()
        }
    }
}

/// Layer description containing its name and bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerSpec {
    pub name: String,
    pub bindings: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
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

#[derive(Deserialize)]
#[serde(untagged)]
enum ComboBindingField {
    Single(String),
    Multiple(Vec<String>),
}

pub(crate) fn deserialize_combo_binding<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<ComboBindingField>::deserialize(deserializer)?;
    Ok(match value {
        Some(ComboBindingField::Single(binding)) => {
            let trimmed = binding.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Some(ComboBindingField::Multiple(list)) => list.into_iter().find_map(|item| {
            let trimmed = item.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }),
        None => None,
    })
}

pub(crate) fn serialize_combo_binding<S>(
    binding: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match binding {
        Some(value) => serializer.serialize_str(value),
        None => serializer.serialize_none(),
    }
}

fn parse_numeric_property(raw: &str) -> Option<u32> {
    let token = raw
        .trim_matches(|ch| matches!(ch, '<' | '>' | ';'))
        .trim()
        .split_whitespace()
        .next()?;
    if let Some(stripped) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        return u32::from_str_radix(stripped, 16).ok();
    }
    if let Some(stripped) = token
        .strip_prefix("0b")
        .or_else(|| token.strip_prefix("0B"))
    {
        return u32::from_str_radix(stripped, 2).ok();
    }
    token.parse::<u32>().ok()
}
