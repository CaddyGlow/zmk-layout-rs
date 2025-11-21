use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Serde mapping of the MoErgo layout JSON payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoergoLayout {
    pub keyboard: Option<String>,
    #[serde(rename = "firmware_api_version", alias = "firmwareApiVersion")]
    pub firmware_api_version: Option<String>,
    pub locale: Option<String>,
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub unlisted: Option<bool>,
    pub date: Option<i64>,
    pub creator: Option<String>,
    pub title: Option<String>,
    pub notes: Option<String>,
    pub tags: Option<Vec<String>>,
    #[serde(default, rename = "custom_defined_behaviors")]
    pub custom_defined_behaviors: String,
    #[serde(default, rename = "custom_devicetree")]
    pub custom_devicetree: String,
    #[serde(default, rename = "config_parameters", alias = "configParameters")]
    pub config_parameters: Option<Value>,
    #[serde(default, rename = "layout_parameters", alias = "layoutParameters")]
    pub layout_parameters: Option<Value>,
    #[serde(default)]
    pub combos: Option<Vec<MoergoCombo>>,
    #[serde(rename = "layer_names", alias = "layerNames")]
    pub layer_names: Vec<String>,
    pub layers: Vec<Vec<MoergoBinding>>,
    #[serde(default)]
    pub macros: Option<Vec<MoergoMacro>>,
    #[serde(rename = "holdTaps", default)]
    pub hold_taps: Option<Vec<MoergoHoldTap>>,
    #[serde(rename = "inputListeners", default)]
    pub input_listeners: Option<Vec<MoergoInputListener>>,
    #[serde(default, rename = "key_position_header", alias = "keyPositionHeader")]
    pub key_position_header: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoergoBinding {
    pub value: Value,
    #[serde(default)]
    pub params: Vec<MoergoBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoergoCombo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub binding: MoergoBinding,
    #[serde(default)]
    pub key_positions: Vec<u32>,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
    #[serde(default)]
    pub layers: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoergoMacro {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub bindings: Vec<MoergoBinding>,
    #[serde(default)]
    pub params: Vec<String>,
    #[serde(default)]
    pub wait_ms: Option<u32>,
    #[serde(default)]
    pub tap_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoergoHoldTap {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub bindings: Vec<String>,
    #[serde(default)]
    pub tapping_term_ms: Option<u32>,
    #[serde(default)]
    pub flavor: Option<String>,
    #[serde(default)]
    pub quick_tap_ms: Option<u32>,
    #[serde(default)]
    pub require_prior_idle_ms: Option<u32>,
    #[serde(default)]
    pub hold_trigger_key_positions: Option<Vec<u32>>,
    #[serde(default)]
    pub hold_trigger_on_release: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoergoInputListener {
    pub code: String,
    #[serde(default)]
    pub input_processors: Vec<MoergoInputProcessor>,
    #[serde(default)]
    pub nodes: Vec<MoergoInputListenerNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoergoInputProcessor {
    pub code: String,
    #[serde(default)]
    pub params: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoergoInputListenerNode {
    pub code: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub layers: Vec<u32>,
    #[serde(default)]
    pub input_processors: Vec<MoergoInputProcessor>,
}
