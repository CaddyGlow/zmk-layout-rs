    use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use thiserror::Error;

use crate::{
    adapters::standard::{
        AdapterLayout, BehaviorSpec, ComboSpec, InputListenerNodeSpec, InputListenerSpec,
        LayoutMetadata, MacroSpec, render_layout_with_template,
    },
    dts::DtsDocument,
};

const DEFAULT_FORMAT_VERSION: &str = "layout-bundle/2025-02-01";
const DEFAULT_MOERGO_TARGET_ID: &str = "moergo";
const DEFAULT_MOERGO_TEMPLATE: &str = "templates/glove80/keymap.dtsi.j2";

/// Describes an adapter-friendly bundle that keeps layout data plus build/context metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutBundle {
    pub format_version: String,
    #[serde(default)]
    pub metadata: BundleMetadata,
    pub layout: AdapterLayout,
    #[serde(default)]
    pub overlays: BundleOverlays,
    #[serde(default)]
    pub symbols: BundleSymbols,
    #[serde(default)]
    pub targets: Vec<BundleTarget>,
    #[serde(default)]
    pub sources: BTreeMap<String, BundleSource>,
}

impl Default for LayoutBundle {
    fn default() -> Self {
        Self {
            format_version: DEFAULT_FORMAT_VERSION.to_string(),
            metadata: BundleMetadata::default(),
            layout: AdapterLayout::default(),
            overlays: BundleOverlays::default(),
            symbols: BundleSymbols::default(),
            targets: Vec::new(),
            sources: BTreeMap::new(),
        }
    }
}

impl LayoutBundle {
    /// Validate bundle structure (format version, target ids, overlay references).
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.format_version != DEFAULT_FORMAT_VERSION {
            return Err(BundleError::Validation(format!(
                "unsupported format_version `{}` (expected `{DEFAULT_FORMAT_VERSION}`)",
                self.format_version
            )));
        }
        let mut ids = HashSet::new();
        for target in &self.targets {
            if !ids.insert(&target.id) {
                return Err(BundleError::Validation(format!(
                    "duplicate target id `{}`",
                    target.id
                )));
            }
            if target
                .template
                .as_ref()
                .map(|path| path.trim().is_empty())
                .unwrap_or(true)
            {
                return Err(BundleError::Validation(format!(
                    "target `{}` is missing a template path",
                    target.id
                )));
            }
            for overlay in &target.overlays {
                if !self.overlays.contains(overlay) {
                    return Err(BundleError::Validation(format!(
                        "target `{}` references unknown overlay `{}`",
                        target.id, overlay
                    )));
                }
            }
            for define in &target.defines {
                if !self.symbols.defines.contains_key(define) {
                    return Err(BundleError::Validation(format!(
                        "target `{}` references unknown define `{}`",
                        target.id, define
                    )));
                }
            }
        }
        Ok(())
    }

    /// Build a bundle from a MoErgo JSON payload.
    pub fn from_moergo_str(json: &str) -> Result<Self, BundleError> {
        let payload: MoergoLayout = serde_json::from_str(json)?;
        let layers = build_layers(&payload);
        let combos = payload
            .combos
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(moergo_combo_to_combo_spec)
            .collect();
        let macros = payload
            .macros
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(moergo_macro_to_macro_spec)
            .collect();
        let behaviors = payload
            .hold_taps
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(moergo_hold_tap_to_behavior_spec)
            .collect();
        let input_listeners = payload
            .input_listeners
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(moergo_input_listener_to_spec)
            .collect();

        let mut layout = AdapterLayout {
            layers,
            combos,
            behaviors,
            macros,
            input_listeners,
            metadata: LayoutMetadata {
                title: payload.title.clone(),
                description: payload.notes.clone(),
                author: payload.creator.clone(),
                version: payload.firmware_api_version.as_ref().map(|v| v.to_string()),
                extras: BTreeMap::new(),
            },
        };
        layout.ensure_property_orders();

        let mut metadata = BundleMetadata::default();
        metadata.title = payload.title.clone();
        metadata.description = payload.notes.clone();
        metadata.keyboard = payload.keyboard.clone();
        metadata.tags = payload.tags.clone().unwrap_or_default();
        metadata
            .extras
            .insert("moergo".into(), moergo_metadata_block(&payload));

        let overlays = BundleOverlays {
            custom_devicetree: non_empty(payload.custom_devicetree.clone()),
            custom_behaviors: non_empty(payload.custom_defined_behaviors.clone()),
            custom_macros: None,
            input_listeners: None,
            fragments: {
                let mut map = BTreeMap::new();
                if let Some(header) = payload.key_position_header.clone() {
                    if !header.trim().is_empty() {
                        map.insert("key_position_header".to_string(), header);
                    }
                }
                map
            },
        };

        let mut symbols = BundleSymbols::default();
        if let Some(locale) = payload.locale.clone() {
            symbols
                .template_vars
                .insert("locale".into(), Value::String(locale));
        }

        let target = BundleTarget {
            id: DEFAULT_MOERGO_TARGET_ID.to_string(),
            kind: Some("moergo".to_string()),
            firmware: Some(BundleFirmware {
                repo: Some("moergo-sc/zmk".to_string()),
                channel: Some("stable".to_string()),
                version: None,
                board: payload.keyboard.clone(),
            }),
            template: Some(DEFAULT_MOERGO_TEMPLATE.to_string()),
            overlays: overlays.default_overlay_names(),
            defines: vec![],
            includes: vec![],
            output: Some(BundleOutput {
                format: "dtsi".to_string(),
            }),
        };

        let mut sources = BTreeMap::new();
        sources.insert(
            "moergo_json".into(),
            BundleSource {
                path: None,
                schema_version: payload.firmware_api_version.clone(),
                fingerprint: None,
                notes: None,
            },
        );

        let bundle = Self {
            format_version: DEFAULT_FORMAT_VERSION.to_string(),
            metadata,
            layout,
            overlays,
            symbols,
            targets: vec![target],
            sources,
        };
        bundle.validate()?;
        Ok(bundle)
    }

    /// Serialize the bundle back into a MoErgo JSON payload.
    pub fn to_moergo_json(&self) -> Result<String, BundleError> {
        let layout = &self.layout;
        let layers = layout
            .layers
            .iter()
            .map(|layer| {
                layer
                    .bindings
                    .iter()
                    .map(|binding| string_binding_to_moergo_binding(binding.as_str()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let combos = layout
            .combos
            .iter()
            .map(combo_spec_to_moergo_combo)
            .collect();
        let macros = layout
            .macros
            .iter()
            .map(macro_spec_to_moergo_macro)
            .collect();
        let hold_taps = extract_hold_taps(layout);

        let input_listeners = layout
            .input_listeners
            .iter()
            .map(spec_to_moergo_input_listener)
            .collect();

        let moergo = MoergoLayout {
            keyboard: self.metadata.keyboard.clone(),
            firmware_api_version: self
                .sources
                .get("moergo_json")
                .and_then(|src| src.schema_version.clone()),
            locale: self
                .symbols
                .template_vars
                .get("locale")
                .and_then(|val| val.as_str().map(|s| s.to_string())),
            uuid: None,
            parent_uuid: None,
            unlisted: None,
            date: None,
            creator: layout.metadata.author.clone(),
            title: layout.metadata.title.clone(),
            notes: layout.metadata.description.clone(),
            tags: Some(self.metadata.tags.clone()),
            custom_defined_behaviors: self.overlays.custom_behaviors.clone().unwrap_or_default(),
            custom_devicetree: self.overlays.custom_devicetree.clone().unwrap_or_default(),
            config_parameters: None,
            layout_parameters: None,
            combos: Some(combos),
            layer_names: layout
                .layers
                .iter()
                .map(|layer| layer.name.clone())
                .collect(),
            layers,
            macros: Some(macros),
            hold_taps: if hold_taps.is_empty() {
                None
            } else {
                Some(hold_taps)
            },
            input_listeners: Some(input_listeners),
            key_position_header: self.overlays.fragments.get("key_position_header").cloned(),
        };
        Ok(serde_json::to_string_pretty(&moergo)?)
    }

    /// Render a target into DTS text using the referenced template.
    pub fn render_target(
        &self,
        target_id: &str,
        template_override: Option<&Path>,
    ) -> Result<String, BundleError> {
        let target = self
            .targets
            .iter()
            .find(|target| target.id == target_id)
            .ok_or_else(|| BundleError::TargetNotFound(target_id.to_string()))?;
        let template_path = if let Some(path) = template_override {
            PathBuf::from(path)
        } else {
            target
                .template
                .as_ref()
                .map(PathBuf::from)
                .ok_or(BundleError::MissingTemplate)?
        };
        let template_source = fs::read_to_string(&template_path)?;
        let mut layout = self.layout.clone();
        apply_overlays_and_symbols(
            &self.metadata,
            &self.overlays,
            &self.symbols,
            target,
            &mut layout,
        )?;
        let rendered = render_layout_with_template(&layout, &template_source)?;
        DtsDocument::parse_str(&rendered)?;
        Ok(rendered)
    }

    pub fn from_moergo_file(path: impl AsRef<Path>) -> Result<Self, BundleError> {
        let json = fs::read_to_string(path)?;
        Self::from_moergo_str(&json)
    }

    pub fn write_json(&self, path: impl AsRef<Path>) -> Result<(), BundleError> {
        let payload = serde_json::to_string_pretty(self)?;
        fs::write(path, payload)?;
        Ok(())
    }

    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self, BundleError> {
        let payload = fs::read_to_string(path)?;
        let bundle: Self = serde_json::from_str(&payload)?;
        bundle.validate()?;
        Ok(bundle)
    }
}

fn apply_overlays_and_symbols(
    metadata: &BundleMetadata,
    overlays: &BundleOverlays,
    symbols: &BundleSymbols,
    target: &BundleTarget,
    layout: &mut AdapterLayout,
) -> Result<(), BundleError> {
    let overlay_set: HashSet<&str> = target
        .overlays
        .iter()
        .map(|name| name.as_str())
        .collect();
    let overlay_selected = |name: &str| overlay_set.contains(name);

    if layout.metadata.title.is_none() {
        layout.metadata.title = metadata.title.clone();
    }
    if layout.metadata.description.is_none() {
        layout.metadata.description = metadata.description.clone();
    }
    let extras = &mut layout.metadata.extras;
    for (key, value) in &metadata.extras {
        extras.entry(key.clone()).or_insert_with(|| value.clone());
    }
    if overlay_selected("custom_devicetree") {
        if let Some(custom) = overlays.custom_devicetree.as_ref() {
        extras.insert("custom_devicetree".into(), Value::String(custom.clone()));
    }
    }
    if overlay_selected("custom_behaviors") {
        if let Some(custom) = overlays.custom_behaviors.as_ref() {
        extras.insert(
            "custom_defined_behaviors".into(),
            Value::String(custom.clone()),
        );
    }
    }
    if overlay_selected("custom_macros") {
        if let Some(custom) = overlays.custom_macros.as_ref() {
        extras.insert(
            "custom_defined_macros".into(),
            Value::String(custom.clone()),
        );
    }
    }
    if overlay_selected("input_listeners") {
        if let Some(listeners) = overlays.input_listeners.as_ref() {
            extras.insert("input_listeners".into(), Value::String(listeners.clone()));
        }
    }
    if let Some(header) = overlays.fragments.get("key_position_header") {
        if overlay_selected("fragments.key_position_header") {
            extras.insert("key_position_header".into(), Value::String(header.clone()));
        }
    }
    let mut includes = symbols.includes.clone();
    includes.extend(target.includes.clone());
    if !includes.is_empty() {
        extras.insert("includes".into(), Value::String(includes.join("\n")));
    }
    for (name, value) in &symbols.template_vars {
        extras.insert(name.clone(), value.clone());
    }
    if let Some(rendered) = render_defines_for_target(symbols, target)? {
        extras.insert("defines".into(), Value::String(rendered));
    }
    Ok(())
}

fn render_defines_for_target(
    symbols: &BundleSymbols,
    target: &BundleTarget,
) -> Result<Option<String>, BundleError> {
    if target.defines.is_empty() {
        return Ok(None);
    }
    let mut lines = Vec::new();
    for name in &target.defines {
        let value = symbols
            .defines
            .get(name)
            .ok_or_else(|| BundleError::Validation(format!(
                "target `{}` references unknown define `{name}`",
                target.id
            )))?;
        lines.push(render_define(name, value));
    }
    Ok(Some(lines.join("\n")))
}

fn render_define(name: &str, value: &Value) -> String {
    let rendered_value = match value {
        Value::Null => None,
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(num) => Some(num.to_string()),
        Value::String(text) => Some(text.to_string()),
        other => Some(other.to_string()),
    };
    match rendered_value {
        Some(val) if val.is_empty() => format!("#define {name}"),
        Some(val) => format!("#define {name} {val}"),
        None => format!("#define {name}"),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub keyboard: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub extras: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleOverlays {
    pub custom_devicetree: Option<String>,
    pub custom_behaviors: Option<String>,
    pub custom_macros: Option<String>,
    pub input_listeners: Option<String>,
    #[serde(default)]
    pub fragments: BTreeMap<String, String>,
}

impl BundleOverlays {
    fn contains(&self, name: &str) -> bool {
        match name {
            "custom_devicetree" => self.custom_devicetree.is_some(),
            "custom_behaviors" => self.custom_behaviors.is_some(),
            "custom_macros" => self.custom_macros.is_some(),
            "input_listeners" => self.input_listeners.is_some(),
            fragment if fragment.starts_with("fragments.") => {
                let key = fragment.trim_start_matches("fragments.");
                self.fragments.contains_key(key)
            }
            _ => false,
        }
    }

    fn default_overlay_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if self.custom_devicetree.is_some() {
            names.push("custom_devicetree".to_string());
        }
        if self.custom_behaviors.is_some() {
            names.push("custom_behaviors".to_string());
        }
        if self.custom_macros.is_some() {
            names.push("custom_macros".to_string());
        }
        if self.input_listeners.is_some() {
            names.push("input_listeners".to_string());
        }
        for key in self.fragments.keys() {
            names.push(format!("fragments.{key}"));
        }
        names
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleSymbols {
    #[serde(default)]
    pub defines: BTreeMap<String, Value>,
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default)]
    pub search_paths: Vec<String>,
    #[serde(default)]
    pub template_vars: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleTarget {
    pub id: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub firmware: Option<BundleFirmware>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub overlays: Vec<String>,
    #[serde(default)]
    pub defines: Vec<String>,
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default)]
    pub output: Option<BundleOutput>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleFirmware {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub board: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleOutput {
    pub format: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleSource {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub schema_version: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Error, Debug)]
pub enum BundleError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("template error: {0}")]
    Template(#[from] crate::adapters::standard::TemplateError),
    #[error("DTS parse error: {0}")]
    Dts(#[from] crate::dts::DtsError),
    #[error("layout parse error: {0}")]
    Layout(#[from] crate::tokenizer::LayoutError),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("target `{0}` not found")]
    TargetNotFound(String),
    #[error("target is missing a template path")]
    MissingTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoergoLayout {
    keyboard: Option<String>,
    #[serde(rename = "firmware_api_version", alias = "firmwareApiVersion")]
    firmware_api_version: Option<String>,
    locale: Option<String>,
    uuid: Option<String>,
    parent_uuid: Option<String>,
    unlisted: Option<bool>,
    date: Option<i64>,
    creator: Option<String>,
    title: Option<String>,
    notes: Option<String>,
    tags: Option<Vec<String>>,
    #[serde(default, rename = "custom_defined_behaviors")]
    custom_defined_behaviors: String,
    #[serde(default, rename = "custom_devicetree")]
    custom_devicetree: String,
    #[serde(default, rename = "config_parameters", alias = "configParameters")]
    config_parameters: Option<Value>,
    #[serde(default, rename = "layout_parameters", alias = "layoutParameters")]
    layout_parameters: Option<Value>,
    #[serde(default)]
    combos: Option<Vec<MoergoCombo>>,
    #[serde(rename = "layer_names", alias = "layerNames")]
    layer_names: Vec<String>,
    layers: Vec<Vec<MoergoBinding>>,
    #[serde(default)]
    macros: Option<Vec<MoergoMacro>>,
    #[serde(rename = "holdTaps", default)]
    hold_taps: Option<Vec<MoergoHoldTap>>,
    #[serde(rename = "inputListeners", default)]
    input_listeners: Option<Vec<MoergoInputListener>>,
    #[serde(default, rename = "key_position_header", alias = "keyPositionHeader")]
    key_position_header: Option<String>,
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
    #[serde(default)]
    key_positions: Vec<u32>,
    #[serde(default)]
    timeout_ms: Option<u32>,
    #[serde(default)]
    layers: Vec<i32>,
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
    #[serde(default)]
    wait_ms: Option<u32>,
    #[serde(default)]
    tap_ms: Option<u32>,
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
    hold_trigger_key_positions: Option<Vec<u32>>,
    #[serde(default)]
    hold_trigger_on_release: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoergoInputListener {
    code: String,
    #[serde(default)]
    input_processors: Vec<MoergoInputProcessor>,
    #[serde(default)]
    nodes: Vec<MoergoInputListenerNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    input_processors: Vec<MoergoInputProcessor>,
}

fn build_layers(payload: &MoergoLayout) -> Vec<crate::adapters::standard::LayerSpec> {
    let mut layers = Vec::new();
    for (idx, name) in payload.layer_names.iter().enumerate() {
        let bindings = payload
            .layers
            .get(idx)
            .map(|row| row.iter().map(binding_to_string).collect())
            .unwrap_or_default();
        layers.push(crate::adapters::standard::LayerSpec {
            name: name.clone(),
            bindings,
        });
    }
    layers
}

fn binding_to_string(binding: &MoergoBinding) -> String {
    let mut parts = Vec::new();
    parts.push(binding_value_to_string(&binding.value));
    for param in &binding.params {
        parts.push(binding_to_string(param));
    }
    parts.join(" ")
}

fn binding_value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(num) => num.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn moergo_combo_to_combo_spec(combo: MoergoCombo) -> ComboSpec {
    ComboSpec {
        name: combo.name,
        description: combo.description.unwrap_or_default(),
        key_positions: combo.key_positions,
        timeout_ms: combo.timeout_ms,
        layers: combo
            .layers
            .into_iter()
            .map(|layer| layer.max(0) as u32)
            .collect(),
        binding: Some(binding_to_string(&combo.binding)),
        behavior: None,
        properties: BTreeMap::new(),
        property_order: Vec::new(),
    }
}

fn moergo_macro_to_macro_spec(m: MoergoMacro) -> MacroSpec {
    MacroSpec {
        name: m.name,
        description: m.description.unwrap_or_default(),
        wait_ms: m.wait_ms,
        tap_ms: m.tap_ms,
        bindings: m
            .bindings
            .into_iter()
            .map(|binding| binding_to_string(&binding))
            .collect(),
        binding_cells: None,
        compatible: None,
        label: None,
        properties: BTreeMap::new(),
        property_order: Vec::new(),
    }
}

fn moergo_hold_tap_to_behavior_spec(ht: MoergoHoldTap) -> BehaviorSpec {
    let mut properties = BTreeMap::new();
    if let Some(term) = ht.tapping_term_ms {
        properties.insert("tapping-term-ms".into(), term.to_string());
    }
    if let Some(flavor) = ht.flavor {
        properties.insert("flavor".into(), flavor);
    }
    if let Some(qt) = ht.quick_tap_ms {
        properties.insert("quick-tap-ms".into(), qt.to_string());
    }
    if let Some(req) = ht.require_prior_idle_ms {
        properties.insert("require-prior-idle-ms".into(), req.to_string());
    }
    if let Some(pos) = ht.hold_trigger_key_positions {
        let rendered = pos
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        properties.insert("hold-trigger-key-positions".into(), rendered);
    }
    if let Some(on_release) = ht.hold_trigger_on_release {
        properties.insert("hold-trigger-on-release".into(), on_release.to_string());
    }
    BehaviorSpec {
        name: ht.name,
        description: ht.description.unwrap_or_default(),
        compatible: Some("zmk,behavior-hold-tap".into()),
        binding_cells: None,
        label: None,
        bindings: ht.bindings,
        properties,
        property_order: Vec::new(),
    }
}

fn moergo_input_listener_to_spec(listener: MoergoInputListener) -> InputListenerSpec {
    InputListenerSpec {
        code: listener.code,
        input_processors: listener
            .input_processors
            .into_iter()
            .map(|proc| crate::adapters::standard::InputProcessorSpec {
                code: proc.code,
                params: proc.params,
            })
            .collect(),
        nodes: listener
            .nodes
            .into_iter()
            .map(|node| InputListenerNodeSpec {
                code: node.code,
                description: node.description,
                layers: node.layers,
                input_processors: node
                    .input_processors
                    .into_iter()
                    .map(|proc| crate::adapters::standard::InputProcessorSpec {
                        code: proc.code,
                        params: proc.params,
                    })
                    .collect(),
                properties: BTreeMap::new(),
                property_order: Vec::new(),
            })
            .collect(),
        properties: BTreeMap::new(),
        property_order: Vec::new(),
    }
}

fn string_binding_to_moergo_binding(binding: &str) -> MoergoBinding {
    let mut tokens = binding
        .split_whitespace()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty());
    let value = tokens.next().unwrap_or_default();
    let params = tokens
        .map(|param| MoergoBinding {
            value: Value::String(param),
            params: Vec::new(),
        })
        .collect();
    MoergoBinding {
        value: Value::String(value),
        params,
    }
}

fn combo_spec_to_moergo_combo(combo: &ComboSpec) -> MoergoCombo {
    MoergoCombo {
        name: combo.name.clone(),
        description: Some(combo.description.clone()),
        binding: combo
            .binding
            .as_ref()
            .map(|binding| string_binding_to_moergo_binding(binding))
            .unwrap_or_else(|| MoergoBinding {
                value: Value::String(String::new()),
                params: Vec::new(),
            }),
        key_positions: combo.key_positions.clone(),
        timeout_ms: combo.timeout_ms,
        layers: combo.layers.iter().map(|layer| *layer as i32).collect(),
    }
}

fn macro_spec_to_moergo_macro(mac: &MacroSpec) -> MoergoMacro {
    MoergoMacro {
        name: mac.name.clone(),
        description: Some(mac.description.clone()),
        bindings: mac
            .bindings
            .iter()
            .map(|binding| string_binding_to_moergo_binding(binding))
            .collect(),
        params: Vec::new(),
        wait_ms: mac.wait_ms,
        tap_ms: mac.tap_ms,
    }
}

fn extract_hold_taps(layout: &AdapterLayout) -> Vec<MoergoHoldTap> {
    layout
        .behaviors
        .iter()
        .filter(|behavior| {
            behavior
                .compatible
                .as_deref()
                .map(|compat: &str| compat.contains("hold-tap"))
                .unwrap_or(false)
        })
        .map(|behavior| {
            let mut ht = MoergoHoldTap {
                name: behavior.name.clone(),
                description: Some(behavior.description.clone()),
                bindings: behavior.bindings.clone(),
                tapping_term_ms: behavior
                    .property_value("tapping-term-ms")
                    .and_then(|v: &str| v.parse::<u32>().ok()),
                flavor: behavior
                    .property_value("flavor")
                    .map(|s: &str| s.to_string()),
                quick_tap_ms: behavior
                    .property_value("quick-tap-ms")
                    .and_then(|v: &str| v.parse::<u32>().ok()),
                require_prior_idle_ms: behavior
                    .property_value("require-prior-idle-ms")
                    .and_then(|v: &str| v.parse::<u32>().ok()),
                hold_trigger_key_positions: behavior
                    .property_value("hold-trigger-key-positions")
                    .map(|v| parse_numbers(v)),
                hold_trigger_on_release: behavior
                    .property_value("hold-trigger-on-release")
                    .map(|v: &str| v.eq_ignore_ascii_case("true")),
            };
            if ht.description.as_ref().is_some_and(|d| d.is_empty()) {
                ht.description = None;
            }
            ht
        })
        .collect()
}

fn parse_numbers(raw: &str) -> Vec<u32> {
    raw.split_whitespace()
        .filter_map(|token| token.parse::<u32>().ok())
        .collect()
}

fn spec_to_moergo_input_listener(spec: &InputListenerSpec) -> MoergoInputListener {
    MoergoInputListener {
        code: spec.code.clone(),
        input_processors: spec
            .input_processors
            .iter()
            .map(|proc| MoergoInputProcessor {
                code: proc.code.clone(),
                params: proc.params.clone(),
            })
            .collect(),
        nodes: spec
            .nodes
            .iter()
            .map(|node| MoergoInputListenerNode {
                code: node.code.clone(),
                description: node.description.clone(),
                layers: node.layers.clone(),
                input_processors: node
                    .input_processors
                    .iter()
                    .map(|proc| MoergoInputProcessor {
                        code: proc.code.clone(),
                        params: proc.params.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn moergo_metadata_block(payload: &MoergoLayout) -> Value {
    let mut map = serde_json::Map::new();
    if let Some(locale) = payload.locale.as_ref() {
        map.insert("locale".into(), Value::String(locale.clone()));
    }
    if let Some(uuid) = payload.uuid.as_ref() {
        map.insert("uuid".into(), Value::String(uuid.clone()));
    }
    if let Some(parent) = payload.parent_uuid.as_ref() {
        map.insert("parent_uuid".into(), Value::String(parent.clone()));
    }
    if let Some(unlisted) = payload.unlisted {
        map.insert("unlisted".into(), Value::Bool(unlisted));
    }
    if let Some(date) = payload.date {
        map.insert("date".into(), Value::Number(date.into()));
    }
    if let Some(params) = payload.config_parameters.as_ref() {
        map.insert("config_parameters".into(), params.clone());
    }
    if let Some(params) = payload.layout_parameters.as_ref() {
        map.insert("layout_parameters".into(), params.clone());
    }
    Value::Object(map)
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}
