//! Simplified adapter that exposes combo/behavior metadata for external tooling.

use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::{self, Number, Value};

use crate::{
    ast::{DtComment, DtItem, DtNode},
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
    #[error(transparent)]
    Template(#[from] TemplateError),
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
}

/// Adapter-friendly view of the parsed layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdapterLayout {
    pub layers: Vec<LayerSpec>,
    pub combos: Vec<ComboSpec>,
    pub behaviors: Vec<BehaviorSpec>,
    pub input_listeners: Vec<InputListenerSpec>,
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
            input_listeners: extract_input_listeners(document),
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

/// Export a document to the standard JSON format while extracting template metadata.
pub fn export_standard_str_with_template(
    document: &DtsDocument,
    rendered_source: &str,
    template_source: &str,
) -> Result<String, AdapterError> {
    let mut layout = AdapterLayout::from_document(document);
    merge_template_metadata(&mut layout, template_source, rendered_source)?;
    Ok(layout.to_standard_json()?)
}

/// Export a DTS file to the standard JSON format using a template for metadata extraction.
pub fn export_standard_file_with_template(
    dts_path: impl AsRef<Path>,
    template_path: impl AsRef<Path>,
    json_path: impl AsRef<Path>,
) -> Result<(), AdapterError> {
    let rendered = fs::read_to_string(&dts_path)?;
    let document = DtsDocument::parse_str(&rendered).map_err(DtsError::from)?;
    let template = fs::read_to_string(template_path)?;
    let json = export_standard_str_with_template(&document, &rendered, &template)?;
    fs::write(json_path, json)?;
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

/// Apply a standard JSON payload to a DTS template provided as a string.
pub fn import_standard_str_with_template(
    json: &str,
    template_source: &str,
) -> Result<DtsDocument, AdapterError> {
    let layout = AdapterLayout::from_standard_json(json)?;
    if !looks_like_template(template_source) {
        let template = DtsDocument::parse_str(template_source).map_err(DtsError::from)?;
        return Ok(layout.apply_to_document(template)?);
    }

    let rendered = render_layout_with_template(&layout, template_source);
    let document = DtsDocument::parse_str(&rendered).map_err(DtsError::from)?;
    Ok(document)
}

/// Read the JSON and template files, generating a new DTS document from both.
pub fn import_standard_file_with_template(
    json_path: impl AsRef<Path>,
    template_path: impl AsRef<Path>,
) -> Result<DtsDocument, AdapterError> {
    let json = fs::read_to_string(json_path)?;
    let template = fs::read_to_string(template_path)?;
    import_standard_str_with_template(&json, &template)
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
            behaviors: layout.behaviors.clone(),
            input_listeners: layout.input_listeners.clone(),
        }
    }
}

impl From<StandardFormat> for AdapterLayout {
    fn from(value: StandardFormat) -> Self {
        Self {
            layers: value.layers,
            combos: value.combos,
            behaviors: value.behaviors,
            input_listeners: value.input_listeners,
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

fn looks_like_template(source: &str) -> bool {
    source.contains("{{") || source.contains("{%")
}

fn render_layout_with_template(layout: &AdapterLayout, template: &str) -> String {
    let replacements = build_template_replacements(layout);
    apply_template(template, &replacements)
}

fn build_template_replacements(layout: &AdapterLayout) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let extras = &layout.metadata.extras;

    let keyboard_name = metadata_text(extras, "keyboard_name")
        .or_else(|| layout.metadata.title.clone())
        .unwrap_or_else(|| "ZMK Layout".to_string());
    insert_placeholder(&mut map, "keyboard_name", keyboard_name);

    let includes = metadata_text(extras, "includes")
        .or_else(|| metadata_text(extras, "resolved_includes"))
        .unwrap_or_default();
    insert_placeholder(&mut map, "includes", includes.clone());
    insert_placeholder(&mut map, "resolved_includes", includes);

    let layer_defines = render_layer_defines(&layout.layers);
    insert_placeholder(&mut map, "layer_names_defines", layer_defines.clone());
    insert_placeholder(&mut map, "layer_defines", layer_defines);

    let rendered_layers = render_layers_only(&layout.layers);
    insert_placeholder(&mut map, "rendered_layers", rendered_layers);

    let keymap_node = render_keymap_node(&layout.layers);
    insert_placeholder(&mut map, "keymap_node", keymap_node);

    let macros_block = render_behaviors(
        layout
            .behaviors
            .iter()
            .filter(|behavior| is_macro_behavior(behavior)),
    );
    insert_placeholder(&mut map, "macros", macros_block.clone());
    insert_placeholder(&mut map, "user_macros_dtsi", macros_block);

    let behaviors_block = render_behaviors(
        layout
            .behaviors
            .iter()
            .filter(|behavior| !is_macro_behavior(behavior)),
    );
    insert_placeholder(&mut map, "behaviors", behaviors_block.clone());
    insert_placeholder(&mut map, "user_behaviors_dtsi", behaviors_block);

    if let Some((combos_root, combos_body)) = render_combos(&layout.combos) {
        insert_placeholder(&mut map, "combos", combos_root);
        insert_placeholder(&mut map, "combos_dtsi", combos_body);
    } else {
        insert_placeholder(&mut map, "combos", String::new());
        insert_placeholder(&mut map, "combos_dtsi", String::new());
    }

    let rendered_input_listeners = render_input_listeners(&layout.input_listeners);
    for key in [
        "custom_devicetree",
        "input_listeners",
        "input_listeners_dtsi",
        "custom_defined_behaviors",
        "input_processors",
        "system_behaviors_dts",
        "key_position_header",
        "custom_defined_macros",
    ] {
        let value = match metadata_text(extras, key) {
            Some(text) => text,
            None if key == "input_listeners" || key == "input_listeners_dtsi" => {
                rendered_input_listeners.clone()
            }
            None => String::new(),
        };
        insert_placeholder(&mut map, key, value);
    }

    map
}

fn insert_placeholder(map: &mut BTreeMap<String, String>, key: &str, value: String) {
    map.insert(key.to_string(), value.clone());
    map.insert(format!("content.{key}"), value);
}

fn metadata_text(extras: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    extras.get(key).map(|value| match value {
        Value::String(text) => text.clone(),
        Value::Number(num) => num.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Number(num) => Some(num.to_string()),
                Value::Bool(flag) => Some(flag.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(obj) => serde_json::to_string_pretty(obj).unwrap_or_default(),
        Value::Null => String::new(),
    })
}

fn render_layer_defines(layers: &[LayerSpec]) -> String {
    if layers.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for (idx, layer) in layers.iter().enumerate() {
        let define_name = sanitize_define_name(&layer.name);
        output.push_str(&format!("#define LAYER_{} {}\n", define_name, idx));
    }
    if output.ends_with('\n') {
        output.pop();
    }
    output
}

fn render_layers_only(layers: &[LayerSpec]) -> String {
    if layers.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for layer in layers {
        output.push_str("        ");
        output.push_str(&layer.name);
        output.push_str(" {\n");
        output.push_str("            bindings = ");
        output.push_str(&format_list(&layer.bindings));
        output.push_str(";\n        };\n");
    }
    output
}

fn render_keymap_node(layers: &[LayerSpec]) -> String {
    let mut output = String::new();
    output.push_str("keymap {\n    compatible = \"zmk,keymap\";\n");
    if !layers.is_empty() {
        let rendered = render_layers_only(layers);
        output.push('\n');
        output.push_str(rendered.trim_end());
        output.push('\n');
    }
    output.push_str("};\n");
    output
}

fn render_behaviors<'a>(behaviors: impl Iterator<Item = &'a BehaviorSpec>) -> String {
    let mut blocks = Vec::new();
    for behavior in behaviors {
        let mut block = String::new();
        block.push_str("        ");
        block.push_str(&behavior.name);
        block.push_str(": ");
        block.push_str(&behavior.name);
        block.push_str(" {\n");
        if let Some(compat) = &behavior.compatible {
            block.push_str("            compatible = \"");
            block.push_str(compat);
            block.push_str("\";\n");
        }
        if let Some(binding_cells) = behavior.binding_cells {
            block.push_str("            #binding-cells = <");
            block.push_str(&binding_cells.to_string());
            block.push_str(">;\n");
        }
        if !behavior.bindings.is_empty() {
            block.push_str("            bindings = ");
            block.push_str(&format_list(&behavior.bindings));
            block.push_str(";\n");
        }
        block.push_str("        };\n");
        blocks.push(block);
    }
    blocks.join("\n")
}

fn render_combos(combos: &[ComboSpec]) -> Option<(String, String)> {
    if combos.is_empty() {
        return None;
    }
    let mut inner = String::new();
    inner.push_str("combos {\n");
    inner.push_str("    compatible = \"zmk,combos\";\n");
    for combo in combos {
        let node_name = sanitize_node_identifier(&combo.name);
        inner.push_str("    combo_");
        inner.push_str(&node_name);
        inner.push_str(" {\n");
        if let Some(timeout) = combo.timeout_ms {
            inner.push_str("        timeout-ms = <");
            inner.push_str(&timeout.to_string());
            inner.push_str(">;\n");
        }
        if !combo.key_positions.is_empty() {
            inner.push_str("        key-positions = <");
            inner.push_str(
                &combo
                    .key_positions
                    .iter()
                    .map(|pos| pos.to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            inner.push_str(">;\n");
        }
        if !combo.bindings.is_empty() {
            inner.push_str("        bindings = ");
            inner.push_str(&format_list(&combo.bindings));
            inner.push_str(";\n");
        }
        inner.push_str("    };\n\n");
    }
    if inner.ends_with("\n\n") {
        inner.truncate(inner.len() - 1);
    }
    inner.push_str("};\n");

    let combos_root = format!("/ {{\n{}\n}};\n", indent_block(inner.trim_end(), 4));
    Some((combos_root, inner))
}

fn render_input_listeners(listeners: &[InputListenerSpec]) -> String {
    if listeners.is_empty() {
        return String::new();
    }
    let mut blocks = Vec::new();
    for listener in listeners {
        let mut block = String::new();
        block.push_str(listener.code.trim());
        block.push_str(" {\n");
        if !listener.input_processors.is_empty() {
            block.push_str("    input-processors = ");
            block.push_str(&render_input_processor_list(&listener.input_processors));
            block.push_str(";\n");
        }
        for node in &listener.nodes {
            if let Some(description) = node.description.as_ref() {
                if !description.trim().is_empty() {
                    block.push_str("    // ");
                    block.push_str(description);
                    block.push('\n');
                }
            }
            block.push_str("    ");
            block.push_str(&node.code);
            block.push_str(" {\n");
            if !node.layers.is_empty() {
                block.push_str("        layers = <");
                block.push_str(
                    &node
                        .layers
                        .iter()
                        .map(|layer| layer.to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                block.push_str(">;\n");
            }
            if !node.input_processors.is_empty() {
                block.push_str("        input-processors = ");
                block.push_str(&render_input_processor_list(&node.input_processors));
                block.push_str(";\n");
            }
            block.push_str("    };\n");
        }
        block.push_str("};\n");
        blocks.push(block);
    }
    blocks.join("\n")
}

fn render_input_processor_list(processors: &[InputProcessorSpec]) -> String {
    processors
        .iter()
        .map(|processor| {
            let mut tokens = Vec::new();
            tokens.push(processor.code.trim().to_string());
            for param in &processor.params {
                let rendered = render_input_processor_param(param);
                if !rendered.is_empty() {
                    tokens.push(rendered);
                }
            }
            format!("<{}>", tokens.join(" "))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_input_processor_param(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(num) => num.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn extract_input_listeners(document: &DtsDocument) -> Vec<InputListenerSpec> {
    let mut listeners = Vec::new();
    collect_input_listener_nodes(&document.items, &mut listeners);
    listeners
}

fn collect_input_listener_nodes(items: &[DtItem], listeners: &mut Vec<InputListenerSpec>) {
    for item in items {
        match item {
            DtItem::Node(node) => {
                if let Some(listener) = build_input_listener_spec(node) {
                    listeners.push(listener);
                }
                collect_input_listener_nodes(&node.children, listeners);
            }
            DtItem::Conditional(cond) => {
                for branch in &cond.branches {
                    collect_input_listener_nodes(&branch.items, listeners);
                }
            }
            _ => {}
        }
    }
}

fn build_input_listener_spec(node: &DtNode) -> Option<InputListenerSpec> {
    if !is_input_listener_node(node) {
        return None;
    }
    let code = listener_identifier(node)?;
    let mut spec = InputListenerSpec {
        code,
        input_processors: parse_input_processors_from_node(node),
        nodes: Vec::new(),
    };
    for child in &node.children {
        if let DtItem::Node(listener_node) = child {
            let node_spec = InputListenerNodeSpec {
                code: listener_node.name.clone(),
                description: extract_description_from_comments(&listener_node.leading_comments),
                layers: parse_layers_property(listener_node),
                input_processors: parse_input_processors_from_node(listener_node),
            };
            spec.nodes.push(node_spec);
        }
    }
    Some(spec)
}

fn is_input_listener_node(node: &DtNode) -> bool {
    let candidates = [node.raw_name.as_str(), node.name.as_str()];
    candidates.iter().any(|candidate| {
        let lowered = candidate.trim().to_ascii_lowercase();
        lowered.contains("input_listener")
    })
}

fn listener_identifier(node: &DtNode) -> Option<String> {
    let candidates = [node.raw_name.as_str(), node.name.as_str()];
    for candidate in candidates {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(start) = trimmed.rfind('&') {
            let remainder = trimmed[start..].split_whitespace().next().unwrap_or("");
            if !remainder.is_empty() {
                return Some(remainder.to_string());
            }
        } else if trimmed.starts_with('&') {
            return Some(trimmed.to_string());
        }
    }
    let fallback = node.name.trim();
    if fallback.is_empty() {
        None
    } else {
        Some(fallback.to_string())
    }
}

fn parse_input_processors_from_node(node: &DtNode) -> Vec<InputProcessorSpec> {
    node.properties
        .iter()
        .find(|prop| prop.name == "input-processors")
        .map(|prop| parse_input_processors_raw(&prop.value.raw))
        .unwrap_or_default()
}

fn parse_input_processors_raw(raw: &str) -> Vec<InputProcessorSpec> {
    split_angle_groups(raw)
        .into_iter()
        .filter_map(|body| parse_input_processor_entry(&body))
        .collect()
}

fn split_angle_groups(raw: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    let bytes = raw.as_bytes();
    for (idx, ch) in bytes.iter().enumerate() {
        match *ch as char {
            '<' => {
                if depth == 0 {
                    start = Some(idx + 1);
                }
                depth += 1;
            }
            '>' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    if let Some(begin) = start.take() {
                        if begin <= idx {
                            groups.push(raw[begin..idx].to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if groups.is_empty() {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            groups.push(trimmed.to_string());
        }
    }
    groups
}

fn parse_input_processor_entry(body: &str) -> Option<InputProcessorSpec> {
    let mut tokens: Vec<&str> = body
        .split_whitespace()
        .map(|token| token.trim_matches(','))
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let code = tokens.remove(0).to_string();
    let params = tokens
        .into_iter()
        .map(parse_input_processor_param_value)
        .collect();
    Some(InputProcessorSpec { code, params })
}

fn parse_input_processor_param_value(token: &str) -> Value {
    let cleaned = token.trim();
    if cleaned.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if cleaned.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if cleaned.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if let Some(stripped) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        if let Ok(value) = u64::from_str_radix(stripped, 16) {
            return Value::Number(Number::from(value));
        }
    }
    if let Some(stripped) = cleaned
        .strip_prefix("0b")
        .or_else(|| cleaned.strip_prefix("0B"))
    {
        if let Ok(value) = u64::from_str_radix(stripped, 2) {
            return Value::Number(Number::from(value));
        }
    }
    if let Ok(value) = cleaned.parse::<i64>() {
        return Value::Number(Number::from(value));
    }
    if cleaned.contains('.') || cleaned.contains('e') || cleaned.contains('E') {
        if let Ok(value) = cleaned.parse::<f64>() {
            if let Some(number) = Number::from_f64(value) {
                return Value::Number(number);
            }
        }
    }
    Value::String(cleaned.to_string())
}

fn parse_layers_property(node: &DtNode) -> Vec<u32> {
    node.properties
        .iter()
        .find(|prop| prop.name == "layers")
        .map(|prop| parse_numeric_list(&prop.value.raw))
        .unwrap_or_default()
}

fn parse_numeric_list(raw: &str) -> Vec<u32> {
    raw.replace('<', " ")
        .replace('>', " ")
        .replace(',', " ")
        .split_whitespace()
        .filter_map(parse_u32_token)
        .collect()
}

fn parse_u32_token(token: &str) -> Option<u32> {
    if token.is_empty() {
        return None;
    }
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

fn extract_description_from_comments(comments: &[DtComment]) -> Option<String> {
    comments
        .iter()
        .rev()
        .find_map(|comment| clean_comment_text(&comment.text))
}

fn clean_comment_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let content = if trimmed.starts_with("//") {
        trimmed
            .trim_start_matches('/')
            .trim_start_matches('/')
            .trim()
    } else if trimmed.starts_with("/*") {
        trimmed
            .trim_start_matches("/*")
            .trim_end_matches("*/")
            .trim()
    } else {
        trimmed
    };
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

fn format_list(items: &[String]) -> String {
    if items.is_empty() {
        "< >".to_string()
    } else {
        format!("< {} >", items.join(" "))
    }
}

fn indent_block(text: &str, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sanitize_define_name(name: &str) -> String {
    let mut result = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_uppercase());
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        "LAYER".to_string()
    } else {
        result
    }
}

fn sanitize_node_identifier(name: &str) -> String {
    let mut result = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            result.push(ch);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        "node".to_string()
    } else {
        result
    }
}

fn is_macro_behavior(behavior: &BehaviorSpec) -> bool {
    behavior
        .compatible
        .as_deref()
        .map(|compat| compat.contains("behavior-macro"))
        .unwrap_or(false)
}

fn apply_template(template: &str, replacements: &BTreeMap<String, String>) -> String {
    let mut output = String::with_capacity(template.len());
    let mut cursor = 0;
    while let Some(start) = template[cursor..].find("{{") {
        let absolute_start = cursor + start;
        output.push_str(&template[cursor..absolute_start]);
        let after_start = absolute_start + 2;
        if let Some(end) = template[after_start..].find("}}") {
            let absolute_end = after_start + end;
            let key = template[after_start..absolute_end].trim();
            let normalized = key.split_whitespace().collect::<String>();
            let content_variant = normalized.strip_prefix("content.").map(|s| s.to_string());
            if let Some(replacement) = replacements.get(&normalized).cloned().or_else(|| {
                content_variant
                    .as_ref()
                    .and_then(|variant| replacements.get(variant).cloned())
            }) {
                output.push_str(&replacement);
                cursor = absolute_end + 2;
            } else {
                output.push_str(&template[absolute_start..absolute_end + 2]);
                cursor = absolute_end + 2;
            }
        } else {
            output.push_str(&template[absolute_start..]);
            return output;
        }
    }
    output.push_str(&template[cursor..]);
    output
}

fn merge_template_metadata(
    layout: &mut AdapterLayout,
    template_source: &str,
    rendered_source: &str,
) -> Result<(), TemplateError> {
    let captured = capture_template_values(template_source, rendered_source)?;
    apply_captured_template_values(layout, captured);
    Ok(())
}

fn apply_captured_template_values(layout: &mut AdapterLayout, captured: BTreeMap<String, String>) {
    const DERIVED_KEYS: &[&str] = &[
        "layer_names_defines",
        "layer_defines",
        "rendered_layers",
        "keymap_node",
        "macros",
        "user_macros_dtsi",
        "behaviors",
        "user_behaviors_dtsi",
        "combos",
        "combos_dtsi",
    ];

    for (key, value) in captured {
        if DERIVED_KEYS.contains(&key.as_str()) {
            continue;
        }
        if key == "keyboard_name" {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                layout.metadata.title = Some(trimmed.to_string());
            }
        }
        layout.metadata.extras.insert(key, Value::String(value));
    }
}

fn capture_template_values(
    template_source: &str,
    rendered_source: &str,
) -> Result<BTreeMap<String, String>, TemplateError> {
    let segments = parse_template_segments(template_source)?;
    let mut cursor = 0;
    let mut pending_placeholder: Option<String> = None;
    let mut captures = BTreeMap::new();

    for segment in segments {
        match segment {
            TemplateSegment::Literal(literal) => {
                if literal.is_empty() {
                    continue;
                }
                let haystack = &rendered_source[cursor..];
                if let Some(offset) = haystack.find(&literal) {
                    let absolute = cursor + offset;
                    if let Some(name) = pending_placeholder.take() {
                        let value = &rendered_source[cursor..absolute];
                        captures.insert(name, value.to_string());
                    } else if absolute != cursor {
                        return Err(TemplateError::UnexpectedContent {
                            literal: snippet(&literal),
                            context: snippet(&rendered_source[cursor..absolute]),
                        });
                    }
                    cursor = absolute + literal.len();
                } else {
                    return Err(TemplateError::LiteralNotFound {
                        literal: snippet(&literal),
                    });
                }
            }
            TemplateSegment::Placeholder(name) => {
                if let Some(previous) = pending_placeholder.replace(name.clone()) {
                    captures.insert(previous, String::new());
                }
            }
        }
    }

    if let Some(name) = pending_placeholder {
        captures.insert(name, rendered_source[cursor..].to_string());
    } else if cursor != rendered_source.len() {
        return Err(TemplateError::TrailingContent {
            trailing: snippet(&rendered_source[cursor..]),
        });
    }

    Ok(captures)
}

fn parse_template_segments(source: &str) -> Result<Vec<TemplateSegment>, TemplateError> {
    let mut segments = Vec::new();
    let mut cursor = 0;
    while let Some(start) = source[cursor..].find("{{") {
        let absolute_start = cursor + start;
        if absolute_start > cursor {
            segments.push(TemplateSegment::Literal(
                source[cursor..absolute_start].to_string(),
            ));
        }
        let after_start = absolute_start + 2;
        let end = source[after_start..]
            .find("}}")
            .map(|offset| after_start + offset)
            .ok_or(TemplateError::UnterminatedPlaceholder {
                offset: absolute_start,
            })?;
        let raw_name = &source[after_start..end];
        let name = canonical_placeholder_name(raw_name);
        segments.push(TemplateSegment::Placeholder(name));
        cursor = end + 2;
    }
    if cursor < source.len() {
        segments.push(TemplateSegment::Literal(source[cursor..].to_string()));
    }
    if segments.is_empty() {
        segments.push(TemplateSegment::Literal(String::new()));
    }
    Ok(segments)
}

fn canonical_placeholder_name(raw: &str) -> String {
    let normalized: String = raw.split_whitespace().collect();
    normalized
        .strip_prefix("content.")
        .map(|value| value.to_string())
        .unwrap_or(normalized)
}

fn snippet(text: &str) -> String {
    let mut sanitized = text.replace('\n', "\\n");
    if sanitized.len() > 40 {
        sanitized.truncate(40);
        sanitized.push('…');
    }
    sanitized
}

#[derive(Debug, Clone)]
enum TemplateSegment {
    Literal(String),
    Placeholder(String),
}

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("failed to parse template: missing closing '}}' for placeholder at byte {offset}")]
    UnterminatedPlaceholder { offset: usize },
    #[error("rendered DTS is missing literal `{literal}` from the template")]
    LiteralNotFound { literal: String },
    #[error("rendered DTS contains unexpected content before literal `{literal}`: `{context}`")]
    UnexpectedContent { literal: String, context: String },
    #[error("rendered DTS has trailing content outside the template: `{trailing}`")]
    TrailingContent { trailing: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn captures_metadata_from_moergo_template() {
        let template = include_str!("../../examples/moergo_glove80.j2");

        let mut layout = AdapterLayout {
            layers: vec![LayerSpec {
                name: "base".into(),
                bindings: vec!["&kp A".into(), "&kp B".into()],
            }],
            combos: Vec::new(),
            behaviors: Vec::new(),
            input_listeners: Vec::new(),
            metadata: LayoutMetadata::default(),
        };
        layout.metadata.title = Some("Glove80".into());
        layout.metadata.extras.insert(
            "includes".into(),
            json!("#include <behaviors.dtsi>\n#include <dt-bindings/zmk/keys.h>"),
        );
        layout.metadata.extras.insert(
            "custom_devicetree".into(),
            json!("&sensor {\n    status = \"okay\";\n};\n"),
        );
        layout
            .metadata
            .extras
            .insert("custom_defined_behaviors".into(), json!("/* custom */\n"));

        let rendered = render_layout_with_template(&layout, template);
        let document = DtsDocument::parse_str(&rendered).expect("template renders valid DTS");
        let mut extracted = AdapterLayout::from_document(&document);
        merge_template_metadata(&mut extracted, template, &rendered)
            .expect("metadata extraction succeeds");

        let includes = extracted
            .metadata
            .extras
            .get("includes")
            .and_then(|value| value.as_str())
            .expect("includes captured");
        assert!(includes.contains("#include <behaviors.dtsi>"));

        let custom_dt = extracted
            .metadata
            .extras
            .get("custom_devicetree")
            .and_then(|value| value.as_str())
            .expect("custom devicetree captured");
        assert!(custom_dt.contains("&sensor"));

        assert_eq!(
            extracted.metadata.title.as_deref(),
            Some("Glove80"),
            "keyboard_name placeholder hydrates title"
        );
    }
}
