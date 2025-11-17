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
        let value = metadata_text(extras, key).unwrap_or_default();
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
