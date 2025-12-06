use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    adapters::{
        standard::{
            BehaviorSpec, ComboSpec, InputListenerNodeSpec, InputListenerSpec, InputProcessorSpec,
            LayerSpec, MacroSpec,
        },
        AdapterError,
    },
    bindings::{BindingParser, LayoutBinding, LayoutParam, ParamValue},
    keymap::{KeymapDocument, KeymapMetadata},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MoergoLayout {
    keyboard: Option<String>,
    firmware_api_version: Option<String>,
    locale: Option<String>,
    uuid: Option<String>,
    parent_uuid: Option<String>,
    #[serde(default)]
    unlisted: bool,
    #[serde(default)]
    date: Option<i64>,
    creator: Option<String>,
    title: Option<String>,
    notes: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    custom_defined_behaviors: String,
    #[serde(default)]
    custom_devicetree: String,
    #[serde(default)]
    config_parameters: Vec<Value>,
    #[serde(default)]
    layout_parameters: Value,
    #[serde(default)]
    layer_names: Vec<String>,
    layers: Vec<Vec<MoergoBinding>>,
    #[serde(default)]
    combos: Vec<MoergoCombo>,
    #[serde(default, rename = "macros")]
    macro_defs: Vec<MoergoMacro>,
    #[serde(default, rename = "holdTaps")]
    hold_taps: Vec<MoergoHoldTap>,
    #[serde(default, rename = "inputListeners")]
    input_listeners: Vec<MoergoInputListener>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MoergoBinding {
    value: Value,
    #[serde(default)]
    params: Vec<MoergoBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoergoCombo {
    name: String,
    #[serde(default)]
    description: Option<String>,
    binding: MoergoBinding,
    key_positions: Vec<u32>,
    #[serde(default)]
    timeout_ms: Option<u32>,
    #[serde(default)]
    layers: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoergoMacro {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    bindings: Vec<MoergoBinding>,
    #[serde(default)]
    params: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoergoHoldTap {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    bindings: Vec<String>,
    #[serde(default)]
    tapping_term_ms: Option<u32>,
    #[serde(default)]
    flavor: Option<String>,
    #[serde(default)]
    quick_tap_ms: Option<u32>,
    #[serde(default)]
    require_prior_idle_ms: Option<u32>,
    #[serde(default)]
    hold_trigger_on_release: Option<bool>,
    #[serde(default)]
    hold_trigger_key_positions: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoergoInputProcessor {
    code: String,
    #[serde(default)]
    params: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoergoInputListenerNode {
    code: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    layers: Vec<u32>,
    #[serde(default, rename = "inputProcessors")]
    input_processors: Vec<MoergoInputProcessor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoergoInputListener {
    code: String,
    #[serde(default, rename = "inputProcessors")]
    input_processors: Vec<MoergoInputProcessor>,
    #[serde(default)]
    nodes: Vec<MoergoInputListenerNode>,
}

pub fn export_moergo_json(document: &KeymapDocument) -> Result<String, AdapterError> {
    let layout = MoergoLayout::from_keymap(document);
    Ok(serde_json::to_string_pretty(&layout)?)
}

pub fn import_moergo_json(json: &str) -> Result<KeymapDocument, AdapterError> {
    let layout: MoergoLayout = serde_json::from_str(json)?;
    let mut keymap = layout.into_keymap();
    keymap
        .metadata
        .extras
        .entry("original_format".into())
        .or_insert(Value::String("moergo-json".into()));
    Ok(keymap)
}

impl MoergoLayout {
    fn from_keymap(document: &KeymapDocument) -> Self {
        let parser = BindingParser::new();
        let layer_names: Vec<String> = document.layers.iter().map(|l| l.name.clone()).collect();
        let layers = document
            .layers
            .iter()
            .map(|layer| {
                layer
                    .bindings
                    .iter()
                    .map(|binding| {
                        let parsed = parser.parse_with_behavior_rules(binding);
                        MoergoBinding::from_layout_binding(&parsed)
                    })
                    .collect()
            })
            .collect();

        let combos = document
            .combos
            .iter()
            .filter_map(|combo| combo.binding.as_ref().map(|_| combo))
            .map(|combo| MoergoCombo::from_combo_spec(combo, &parser))
            .collect();

        let macro_defs = document
            .macros
            .iter()
            .map(|mac| MoergoMacro::from_macro_spec(mac, &parser))
            .collect();

        let hold_taps = document
            .behaviors
            .iter()
            .filter(|behavior| is_hold_tap_behavior(behavior))
            .map(MoergoHoldTap::from_behavior_spec)
            .collect();

        let input_listeners = document
            .input_listeners
            .iter()
            .map(MoergoInputListener::from_spec)
            .collect();

        let metadata = &document.metadata;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|dur| dur.as_secs() as i64)
            .unwrap_or_default();
        let (
            keyboard,
            firmware_api_version,
            locale,
            uuid,
            parent_uuid,
            unlisted,
            date,
            tags,
            cdb,
            cdt,
            cfg,
            layout_params,
        ) = extract_extras(metadata);

        Self {
            keyboard: keyboard.or_else(|| Some("glove80".into())),
            firmware_api_version: firmware_api_version.or_else(|| Some("1".into())),
            locale: locale.or_else(|| Some("en-US".into())),
            uuid,
            parent_uuid,
            unlisted: unlisted.unwrap_or(false),
            date: Some(date.unwrap_or(now)),
            creator: metadata.author.clone(),
            title: metadata.title.clone(),
            notes: metadata.description.clone(),
            tags,
            custom_defined_behaviors: cdb.unwrap_or_default(),
            custom_devicetree: cdt.unwrap_or_default(),
            config_parameters: cfg.unwrap_or_default(),
            layout_parameters: layout_params.unwrap_or_else(|| Value::Object(Default::default())),
            layer_names,
            layers,
            combos,
            macro_defs,
            hold_taps,
            input_listeners,
        }
    }

    fn into_keymap(self) -> KeymapDocument {
        let extras = build_extras_from_moergo(&self);
        let layers = self
            .layers
            .into_iter()
            .enumerate()
            .map(|(idx, layer)| {
                let name = self
                    .layer_names
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| format!("layer_{idx}"));
                let bindings = layer
                    .iter()
                    .map(|binding| binding.to_binding_string())
                    .collect();
                LayerSpec {
                    name,
                    bindings,
                    properties: BTreeMap::new(),
                }
            })
            .collect();

        let combos = self
            .combos
            .into_iter()
            .map(|combo| combo.into_combo_spec())
            .collect();

        let macros = self
            .macro_defs
            .into_iter()
            .map(|mac| mac.into_macro_spec())
            .collect();

        let behaviors = self
            .hold_taps
            .into_iter()
            .map(|ht| ht.into_behavior_spec())
            .collect();

        let input_listeners = self
            .input_listeners
            .into_iter()
            .map(|listener| listener.into_spec())
            .collect();

        let metadata = KeymapMetadata {
            title: self.title,
            description: self.notes,
            author: self.creator,
            version: None,
            extras,
            template: None,
        };

        KeymapDocument {
            layers,
            combos,
            behaviors,
            macros,
            input_listeners,
            metadata,
        }
    }
}

impl MoergoBinding {
    fn from_layout_binding(binding: &LayoutBinding) -> Self {
        // Strip leading '&' only for Custom behaviors (moergo format uses "Custom" not "&Custom")
        let is_custom = binding.value == "&Custom";
        let value = if is_custom {
            "Custom".to_string()
        } else {
            binding.value.clone()
        };

        // For Custom bindings, flatten params to single strings (don't decompose nested bindings)
        let params = if is_custom {
            binding
                .params
                .iter()
                .map(|param| MoergoBinding {
                    value: Value::String(stringify_layout_param(param)),
                    params: vec![],
                })
                .collect()
        } else {
            binding
                .params
                .iter()
                .map(MoergoBinding::from_layout_param)
                .collect()
        };

        Self {
            value: Value::String(value),
            params,
        }
    }

    fn from_layout_param(param: &LayoutParam) -> Self {
        let value = match &param.value {
            ParamValue::Text(text) => Value::String(text.clone()),
            ParamValue::Integer(num) => Value::Number((*num).into()),
        };
        let params = param
            .params
            .iter()
            .map(MoergoBinding::from_layout_param)
            .collect();
        Self { value, params }
    }

    fn to_binding_string(&self) -> String {
        let value = value_to_string(&self.value);
        // Add '&' prefix for "Custom" behavior when importing from moergo format
        let value = if value == "Custom" {
            "&Custom".to_string()
        } else {
            value
        };
        if self.params.is_empty() {
            return value;
        }
        let joined = self
            .params
            .iter()
            .map(|param| param.to_binding_string())
            .collect::<Vec<_>>()
            .join(" ");
        format!("{value} {joined}")
    }
}

impl MoergoCombo {
    fn from_combo_spec(spec: &ComboSpec, parser: &BindingParser) -> Self {
        let binding = spec
            .binding
            .as_deref()
            .map(|binding| {
                let parsed = parser.parse_with_behavior_rules(binding);
                MoergoBinding::from_layout_binding(&parsed)
            })
            .unwrap_or_else(|| MoergoBinding {
                value: Value::String("&none".into()),
                params: Vec::new(),
            });
        Self {
            name: spec.name.clone(),
            description: if spec.description.is_empty() {
                None
            } else {
                Some(spec.description.clone())
            },
            binding,
            key_positions: spec.key_positions.clone(),
            timeout_ms: spec.timeout_ms,
            layers: spec.layers.clone(),
        }
    }

    fn into_combo_spec(self) -> ComboSpec {
        let binding = Some(self.binding.to_binding_string());
        ComboSpec {
            name: self.name,
            description: self.description.unwrap_or_default(),
            key_positions: self.key_positions,
            timeout_ms: self.timeout_ms,
            layers: self.layers,
            binding,
            behavior: None,
            properties: BTreeMap::new(),
            conditions: Vec::new(),
            property_order: Vec::new(),
        }
    }
}

impl MoergoMacro {
    fn from_macro_spec(spec: &MacroSpec, parser: &BindingParser) -> Self {
        let bindings = spec
            .bindings
            .iter()
            .map(|binding| {
                let parsed = parser.parse_with_behavior_rules(binding);
                MoergoBinding::from_layout_binding(&parsed)
            })
            .collect();
        let params = if let Some(cells) = spec.binding_cells {
            (1..=cells).map(|idx| format!("param{idx}")).collect()
        } else {
            Vec::new()
        };
        Self {
            name: spec.name.clone(),
            description: if spec.description.is_empty() {
                None
            } else {
                Some(spec.description.clone())
            },
            bindings,
            params,
        }
    }

    fn into_macro_spec(self) -> MacroSpec {
        MacroSpec {
            name: self.name,
            description: self.description.unwrap_or_default(),
            wait_ms: None,
            tap_ms: None,
            bindings: self
                .bindings
                .iter()
                .map(|binding| binding.to_binding_string())
                .collect(),
            binding_cells: if self.params.is_empty() {
                None
            } else {
                Some(self.params.len() as u32)
            },
            compatible: None,
            label: None,
            properties: BTreeMap::new(),
            property_order: Vec::new(),
        }
    }
}

impl MoergoHoldTap {
    fn from_behavior_spec(spec: &BehaviorSpec) -> Self {
        Self {
            name: spec.name.clone(),
            description: if spec.description.is_empty() {
                None
            } else {
                Some(spec.description.clone())
            },
            bindings: spec.bindings.clone(),
            tapping_term_ms: parse_u32_property(spec, "tapping-term-ms"),
            flavor: spec.properties.get("flavor").cloned(),
            quick_tap_ms: parse_u32_property(spec, "quick-tap-ms"),
            require_prior_idle_ms: parse_u32_property(spec, "require-prior-idle-ms"),
            hold_trigger_on_release: spec
                .properties
                .get("hold-trigger-on-release")
                .and_then(|v| v.parse::<bool>().ok()),
            hold_trigger_key_positions: parse_u32_list(
                spec.properties
                    .get("hold-trigger-key-positions")
                    .map(String::as_str)
                    .unwrap_or(""),
            ),
        }
    }

    fn into_behavior_spec(self) -> BehaviorSpec {
        let mut properties = BTreeMap::new();
        if let Some(value) = self.tapping_term_ms {
            properties.insert("tapping-term-ms".into(), value.to_string());
        }
        if let Some(value) = self.flavor {
            properties.insert("flavor".into(), value);
        }
        if let Some(value) = self.quick_tap_ms {
            properties.insert("quick-tap-ms".into(), value.to_string());
        }
        if let Some(value) = self.require_prior_idle_ms {
            properties.insert("require-prior-idle-ms".into(), value.to_string());
        }
        if let Some(value) = self.hold_trigger_on_release {
            properties.insert("hold-trigger-on-release".into(), value.to_string());
        }
        if !self.hold_trigger_key_positions.is_empty() {
            let rendered = self
                .hold_trigger_key_positions
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            properties.insert("hold-trigger-key-positions".into(), rendered);
        }

        BehaviorSpec {
            name: self.name,
            description: self.description.unwrap_or_default(),
            compatible: Some("zmk,hold-tap".into()),
            binding_cells: Some(self.bindings.len() as u32),
            label: None,
            bindings: self.bindings,
            properties,
            property_order: Vec::new(),
        }
    }
}

impl MoergoInputListener {
    fn from_spec(spec: &InputListenerSpec) -> Self {
        let nodes = spec
            .nodes
            .iter()
            .map(MoergoInputListenerNode::from_spec)
            .collect();
        let input_processors = spec
            .input_processors
            .iter()
            .map(MoergoInputProcessor::from_spec)
            .collect();
        Self {
            code: spec.code.clone(),
            input_processors,
            nodes,
        }
    }

    fn into_spec(self) -> InputListenerSpec {
        let nodes = self
            .nodes
            .into_iter()
            .map(MoergoInputListenerNode::into_spec)
            .collect();
        let input_processors = self
            .input_processors
            .into_iter()
            .map(MoergoInputProcessor::into_spec)
            .collect();
        InputListenerSpec {
            code: self.code,
            input_processors,
            nodes,
            properties: BTreeMap::new(),
            property_order: Vec::new(),
        }
    }
}

impl MoergoInputListenerNode {
    fn from_spec(spec: &InputListenerNodeSpec) -> Self {
        let input_processors = spec
            .input_processors
            .iter()
            .map(MoergoInputProcessor::from_spec)
            .collect();
        Self {
            code: spec.code.clone(),
            description: spec.description.clone(),
            layers: spec.layers.clone(),
            input_processors,
        }
    }

    fn into_spec(self) -> InputListenerNodeSpec {
        let input_processors = self
            .input_processors
            .into_iter()
            .map(MoergoInputProcessor::into_spec)
            .collect();
        InputListenerNodeSpec {
            code: self.code,
            description: self.description,
            layers: self.layers,
            input_processors,
            properties: BTreeMap::new(),
            property_order: Vec::new(),
        }
    }
}

impl MoergoInputProcessor {
    fn from_spec(spec: &InputProcessorSpec) -> Self {
        Self {
            code: spec.code.clone(),
            params: spec.params.clone(),
        }
    }

    fn into_spec(self) -> InputProcessorSpec {
        InputProcessorSpec {
            code: self.code,
            params: self.params,
        }
    }
}

fn stringify_layout_param(param: &LayoutParam) -> String {
    let value = match &param.value {
        ParamValue::Text(text) => text.clone(),
        ParamValue::Integer(num) => num.to_string(),
    };
    // Strip unnecessary parentheses from simple identifiers (e.g., "(LEFT_PINKY_MOD)" -> "LEFT_PINKY_MOD")
    let value = strip_simple_parens(&value);
    if param.params.is_empty() {
        return value;
    }
    let joined = param
        .params
        .iter()
        .map(stringify_layout_param)
        .collect::<Vec<_>>()
        .join(" ");
    format!("{value} {joined}")
}

/// Strip outer parentheses from a value if they're just for grouping.
/// E.g., "(LEFT_PINKY_MOD)" -> "LEFT_PINKY_MOD"
/// E.g., "(_C(L))" -> "_C(L)" (function call)
/// But keep parens for operator expressions like "(A | B)" or "(1 + 2)"
fn strip_simple_parens(value: &str) -> String {
    if value.starts_with('(') && value.ends_with(')') {
        let inner = &value[1..value.len() - 1];
        // Strip if it's a simple identifier or a function call without operators
        if is_strippable_expression(inner) {
            return inner.to_string();
        }
    }
    value.to_string()
}

/// Check if the expression can have its outer parens stripped.
/// Returns true for simple identifiers and function calls without operators.
fn is_strippable_expression(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Check for binary operators that require parens to be kept
    let has_operator = s.contains(" | ")
        || s.contains(" & ")
        || s.contains(" ^ ")
        || s.contains(" + ")
        || s.contains(" - ")
        || s.contains(" * ")
        || s.contains(" / ")
        || s.contains(" << ")
        || s.contains(" >> ");
    if has_operator {
        return false;
    }
    // Make sure parentheses are balanced (for nested function calls like _C(L))
    let mut depth = 0;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn parse_u32_property(spec: &BehaviorSpec, key: &str) -> Option<u32> {
    spec.properties
        .get(key)
        .and_then(|value| value.parse::<u32>().ok())
}

fn parse_u32_list(raw: &str) -> Vec<u32> {
    raw.trim_matches(|ch| matches!(ch, '<' | '>' | ';'))
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter_map(|token| token.parse::<u32>().ok())
        .collect()
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(num) => num.to_string(),
        Value::Bool(flag) => flag.to_string(),
        other => other.to_string(),
    }
}

fn is_hold_tap_behavior(behavior: &BehaviorSpec) -> bool {
    behavior
        .compatible
        .as_deref()
        .map(|comp| comp.contains("hold-tap"))
        .unwrap_or(false)
        || behavior.properties.contains_key("tapping-term-ms")
}

fn extract_extras(
    metadata: &KeymapMetadata,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<bool>,
    Option<i64>,
    Vec<String>,
    Option<String>,
    Option<String>,
    Option<Vec<Value>>,
    Option<Value>,
) {
    (
        metadata
            .extras
            .get("keyboard")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        metadata
            .extras
            .get("firmware_api_version")
            .or_else(|| metadata.extras.get("firmware-api-version"))
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        metadata
            .extras
            .get("locale")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        metadata
            .extras
            .get("uuid")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        metadata
            .extras
            .get("parent_uuid")
            .or_else(|| metadata.extras.get("parent-uuid"))
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        metadata.extras.get("unlisted").and_then(|v| v.as_bool()),
        metadata.extras.get("date").and_then(|v| v.as_i64()),
        metadata
            .extras
            .get("tags")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| entry.as_str().map(|s| s.to_string()))
            .collect(),
        metadata
            .extras
            .get("custom_defined_behaviors")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        metadata
            .extras
            .get("custom_devicetree")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        metadata
            .extras
            .get("config_parameters")
            .and_then(|v| v.as_array().cloned()),
        metadata.extras.get("layout_parameters").cloned(),
    )
}

fn build_extras_from_moergo(layout: &MoergoLayout) -> BTreeMap<String, Value> {
    let mut extras = BTreeMap::new();
    extras.insert(
        "keyboard".into(),
        Value::String(layout.keyboard.clone().unwrap_or_else(|| "glove80".into())),
    );
    extras.insert(
        "firmware_api_version".into(),
        Value::String(
            layout
                .firmware_api_version
                .clone()
                .unwrap_or_else(|| "1".into()),
        ),
    );
    extras.insert(
        "locale".into(),
        Value::String(layout.locale.clone().unwrap_or_else(|| "en-US".into())),
    );
    if let Some(uuid) = &layout.uuid {
        extras.insert("uuid".into(), Value::String(uuid.clone()));
    }
    if let Some(parent_uuid) = &layout.parent_uuid {
        extras.insert("parent_uuid".into(), Value::String(parent_uuid.clone()));
    }
    extras.insert("unlisted".into(), Value::Bool(layout.unlisted));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs() as i64)
        .unwrap_or_default();
    extras.insert(
        "date".into(),
        Value::Number(layout.date.unwrap_or(now).into()),
    );
    extras.insert(
        "tags".into(),
        Value::Array(
            layout
                .tags
                .iter()
                .map(|t| Value::String(t.clone()))
                .collect(),
        ),
    );
    extras.insert(
        "custom_defined_behaviors".into(),
        Value::String(layout.custom_defined_behaviors.clone()),
    );
    extras.insert(
        "custom_devicetree".into(),
        Value::String(layout.custom_devicetree.clone()),
    );
    extras.insert(
        "config_parameters".into(),
        Value::Array(layout.config_parameters.clone()),
    );
    extras.insert(
        "layout_parameters".into(),
        if layout.layout_parameters.is_null() {
            Value::Object(Default::default())
        } else {
            layout.layout_parameters.clone()
        },
    );
    extras
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_export_strips_ampersand_from_custom() {
        let parser = BindingParser::new();
        let parsed = parser.parse_with_behavior_rules("&Custom foo");
        let mo = MoergoBinding::from_layout_binding(&parsed);
        // Moergo format uses "Custom" without '&' prefix
        assert_eq!(value_to_string(&mo.value), "Custom");
    }

    #[test]
    fn binding_export_preserves_other_behaviors() {
        let parser = BindingParser::new();
        let parsed = parser.parse_with_behavior_rules("&kp A");
        let mo = MoergoBinding::from_layout_binding(&parsed);
        // Other behaviors keep their '&' prefix
        assert_eq!(value_to_string(&mo.value), "&kp");
    }

    #[test]
    fn binding_import_adds_ampersand_to_custom() {
        let mo = MoergoBinding {
            value: Value::String("Custom".into()),
            params: vec![MoergoBinding {
                value: Value::String("foo".into()),
                params: vec![],
            }],
        };
        // Import adds '&' prefix to Custom
        assert_eq!(mo.to_binding_string(), "&Custom foo");
    }

    #[test]
    fn binding_round_trip_preserves_value() {
        let parser = BindingParser::new();
        let parsed = parser.parse_with_behavior_rules("&mt LSHFT A");
        let mo = MoergoBinding::from_layout_binding(&parsed);
        // Round trip: internal -> moergo -> internal
        assert_eq!(mo.to_binding_string(), "&mt LSHFT A");
    }

    #[test]
    fn custom_binding_round_trip() {
        let parser = BindingParser::new();
        let parsed = parser.parse_with_behavior_rules("&Custom foo bar");
        let mo = MoergoBinding::from_layout_binding(&parsed);
        // Export should strip '&' from Custom
        assert_eq!(value_to_string(&mo.value), "Custom");
        // Import should add it back
        assert_eq!(mo.to_binding_string(), "&Custom foo bar");
    }

    #[test]
    fn custom_binding_flattens_nested_params() {
        let parser = BindingParser::new();
        let parsed = parser.parse_with_behavior_rules("&Custom &left_index_tap KEY_LH_C1R4");
        let mo = MoergoBinding::from_layout_binding(&parsed);
        // Custom params should be flattened to single strings, not nested
        assert_eq!(mo.params.len(), 1);
        assert_eq!(
            value_to_string(&mo.params[0].value),
            "&left_index_tap KEY_LH_C1R4"
        );
        assert!(mo.params[0].params.is_empty());
    }

    #[test]
    fn custom_binding_strips_simple_parens() {
        let parser = BindingParser::new();
        // Input with function-call syntax: &kp(LEFT_PINKY_MOD)
        let parsed = parser.parse_with_behavior_rules("&Custom &kp(LEFT_PINKY_MOD)");
        let mo = MoergoBinding::from_layout_binding(&parsed);
        // Should strip unnecessary parens from simple identifier
        assert_eq!(
            value_to_string(&mo.params[0].value),
            "&kp LEFT_PINKY_MOD"
        );
    }

    #[test]
    fn strip_simple_parens_keeps_complex_expressions() {
        // Simple identifier - strip parens
        assert_eq!(strip_simple_parens("(FOO)"), "FOO");
        assert_eq!(strip_simple_parens("(LEFT_PINKY_MOD)"), "LEFT_PINKY_MOD");

        // Function calls - strip outer parens
        assert_eq!(strip_simple_parens("(_C(L))"), "_C(L)");
        assert_eq!(strip_simple_parens("(_C(K))"), "_C(K)");
        assert_eq!(strip_simple_parens("(foo(bar))"), "foo(bar)");

        // Operator expressions - keep parens
        assert_eq!(strip_simple_parens("(A | B)"), "(A | B)");
        assert_eq!(strip_simple_parens("(1 + 2)"), "(1 + 2)");
        assert_eq!(strip_simple_parens("(A << 8)"), "(A << 8)");

        // No parens - unchanged
        assert_eq!(strip_simple_parens("FOO"), "FOO");
        assert_eq!(strip_simple_parens("_C(L)"), "_C(L)");
    }
}
