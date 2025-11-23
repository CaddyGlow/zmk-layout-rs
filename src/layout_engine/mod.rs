//! Layout mutation helpers shared across tasks, scripts, and future tooling.

use crate::{
    ast::{DtItem, DtNode, DtProperty, DtValue},
    bindings::BindingParser,
    dts::DtsDocument,
    providers::{COMBO_CONDITION_COMMENT_PREFIX, KeymapDocument, ProviderError},
    tokenizer::TokenSpan,
};
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;
use toml::Value as TomlValue;

/// Generic map used throughout layout customization metadata.
pub type MetadataMap = BTreeMap<String, TomlValue>;

/// Layer selection helper shared by combo definitions and scripts.
#[derive(Debug, Clone)]
pub enum LayerSelector {
    Index(u32),
    Name(String),
}

/// Result of invoking layout mutations.
#[derive(Debug, Error)]
pub enum LayoutEngineError {
    #[error("{0}")]
    Provider(#[from] ProviderError),
    #[error("{0}")]
    Validation(String),
}

/// Convenience wrapper around [`KeymapDocument`] that centralizes editing logic.
#[derive(Debug)]
pub struct LayoutEngine {
    document: KeymapDocument,
    parser: BindingParser,
}

impl Clone for LayoutEngine {
    fn clone(&self) -> Self {
        Self {
            document: self.document.clone(),
            parser: BindingParser::new(),
        }
    }
}

impl LayoutEngine {
    pub fn new(document: KeymapDocument) -> Self {
        Self {
            document,
            parser: BindingParser::new(),
        }
    }

    pub fn into_document(self) -> KeymapDocument {
        self.document
    }

    pub fn document(&self) -> &KeymapDocument {
        &self.document
    }

    pub fn layer_names(&self) -> Vec<String> {
        self.document.layer_names()
    }

    pub fn layer_to_string(&self, layer: &str) -> Option<String> {
        layer_to_string(self.document.document(), layer)
    }

    pub fn combo_to_string(&self, combo: &str) -> Option<String> {
        combo_to_string(self.document.document(), combo)
    }

    pub fn layer_order_to_string(&self) -> String {
        layer_order_to_string(self.document.document())
    }

    pub fn layer_bindings(&self, layer: &str) -> Result<Vec<String>, LayoutEngineError> {
        let entries = self.document.bindings_for_layer(layer)?;
        Ok(entries
            .into_iter()
            .map(|entry| entry.to_binding_string())
            .collect())
    }

    pub fn normalize_binding(&mut self, value: &str) -> Result<String, LayoutEngineError> {
        if value.trim().is_empty() {
            return Err(LayoutEngineError::Validation(
                "binding value cannot be empty".into(),
            ));
        }
        Ok(self
            .parser
            .parse_with_behavior_rules(value)
            .to_binding_string())
    }

    pub fn normalize_binding_list(
        &mut self,
        bindings: &[String],
    ) -> Result<Vec<String>, LayoutEngineError> {
        if bindings.is_empty() {
            return Err(LayoutEngineError::Validation(
                "layer must define at least one binding".into(),
            ));
        }
        bindings
            .iter()
            .map(|binding| self.normalize_binding(binding))
            .collect()
    }

    pub fn set_binding(
        &mut self,
        layer: &str,
        index: usize,
        binding: &str,
    ) -> Result<(), LayoutEngineError> {
        self.document
            .set_binding(layer, index, binding)
            .map_err(LayoutEngineError::from)
    }

    pub fn set_layer_bindings(
        &mut self,
        layer: &str,
        bindings: &[String],
    ) -> Result<(), LayoutEngineError> {
        let refs: Vec<&str> = bindings.iter().map(|binding| binding.as_str()).collect();
        self.document
            .set_layer_bindings(layer, &refs)
            .map_err(LayoutEngineError::from)
    }

    pub fn set_layer_metadata(
        &mut self,
        layer: &str,
        metadata: &[(String, String)],
    ) -> Result<(), LayoutEngineError> {
        self.document
            .set_layer_metadata(layer, metadata)
            .map_err(LayoutEngineError::from)
    }

    pub fn upsert_combo(
        &mut self,
        name: &str,
        binding: &str,
        key_positions: &[u32],
        timeout_ms: Option<u32>,
        layers: &[u32],
        conditions: &[String],
    ) -> Result<(), LayoutEngineError> {
        self.document
            .upsert_combo(name, binding, key_positions, timeout_ms, layers, conditions)
            .map_err(LayoutEngineError::from)
    }

    pub fn set_behavior_bindings(
        &mut self,
        behavior: &str,
        bindings: &[String],
    ) -> Result<(), LayoutEngineError> {
        if bindings.is_empty() {
            return Err(LayoutEngineError::Validation(
                "behavior bindings cannot be empty".into(),
            ));
        }
        let refs: Vec<&str> = bindings.iter().map(|binding| binding.as_str()).collect();
        self.document
            .set_behavior_bindings(behavior, &refs)
            .map_err(LayoutEngineError::from)
    }

    pub fn set_behavior_properties(
        &mut self,
        behavior: &str,
        properties: &[(String, String)],
    ) -> Result<(), LayoutEngineError> {
        if properties.is_empty() {
            return Ok(());
        }
        self.document
            .set_behavior_properties(behavior, properties)
            .map_err(LayoutEngineError::from)
    }

    pub fn set_behavior_settings(
        &mut self,
        behavior: &str,
        settings: &MetadataMap,
    ) -> Result<(), LayoutEngineError> {
        if settings.is_empty() {
            return Ok(());
        }
        let mut bindings = Vec::new();
        let mut properties = Vec::new();
        for (key, value) in settings {
            if key == "bindings" {
                let array = match value {
                    TomlValue::Array(items) => items,
                    _ => {
                        return Err(LayoutEngineError::Validation(
                            "behavior bindings must be an array of strings".into(),
                        ));
                    }
                };
                if array.is_empty() {
                    return Err(LayoutEngineError::Validation(
                        "behavior bindings must contain at least one entry".into(),
                    ));
                }
                bindings = array_to_binding_strings(array)?;
            } else {
                properties.push((key.clone(), format_metadata_value(value)));
            }
        }
        if !bindings.is_empty() {
            let mut normalized = Vec::with_capacity(bindings.len());
            for binding in bindings {
                normalized.push(self.normalize_binding(&binding)?);
            }
            self.set_behavior_bindings(behavior, &normalized)?;
        }
        if !properties.is_empty() {
            self.set_behavior_properties(behavior, &properties)?;
        }
        Ok(())
    }

    pub fn reorder_layer(&mut self, layer: &str, index: usize) -> Result<(), LayoutEngineError> {
        self.document
            .reorder_layer(layer, index)
            .map_err(LayoutEngineError::from)
    }

    pub fn resolve_layer_selectors(
        &self,
        selectors: &[LayerSelector],
    ) -> Result<Vec<u32>, LayoutEngineError> {
        if selectors.is_empty() {
            return Ok(Vec::new());
        }
        let names = self.layer_names();
        let mut map = HashMap::new();
        for (index, name) in names.iter().enumerate() {
            map.insert(name.clone(), index as u32);
        }
        let mut result = Vec::with_capacity(selectors.len());
        for selector in selectors {
            match selector {
                LayerSelector::Index(value) => {
                    if (*value as usize) < names.len() {
                        result.push(*value);
                    } else {
                        return Err(LayoutEngineError::Validation(format!(
                            "layer index {} out of range (len {})",
                            value,
                            names.len()
                        )));
                    }
                }
                LayerSelector::Name(name) => {
                    if let Some(index) = map.get(name) {
                        result.push(*index);
                    } else {
                        return Err(LayoutEngineError::Validation(format!(
                            "layer `{}` not found for combo",
                            name
                        )));
                    }
                }
            }
        }
        Ok(result)
    }

    pub fn metadata_to_properties(metadata: &MetadataMap) -> Vec<(String, String)> {
        metadata
            .iter()
            .map(|(key, value)| (key.clone(), format_metadata_value(value)))
            .collect()
    }

    pub fn behavior_to_string(&self, behavior: &str) -> Option<String> {
        behavior_to_string(self.document.document(), behavior)
    }

    pub fn meta_to_string(&self, key: &str) -> Option<String> {
        meta_to_string(self.document.document(), key)
    }

    pub fn set_meta_entry(
        &mut self,
        key: &str,
        value: &TomlValue,
    ) -> Result<(), LayoutEngineError> {
        let formatted = format_metadata_value(value);
        set_meta_property(self.document.document_mut(), key, formatted);
        Ok(())
    }

    /// Add a new layer with the given name and bindings.
    ///
    /// # Arguments
    /// * `name` - The name of the new layer
    /// * `bindings` - The key bindings for the layer
    ///
    /// # Returns
    /// * `Ok(())` if successful
    /// * `Err(LayoutEngineError)` if layer already exists or bindings are invalid
    pub fn add_layer(&mut self, name: &str, bindings: &[String]) -> Result<(), LayoutEngineError> {
        // Check if layer already exists
        if self.layer_names().contains(&name.to_string()) {
            return Err(LayoutEngineError::Validation(format!(
                "layer '{}' already exists",
                name
            )));
        }

        // Validate bindings
        if bindings.is_empty() {
            return Err(LayoutEngineError::Validation(
                "layer must have at least one binding".into(),
            ));
        }

        // Add the layer using the provider
        let refs: Vec<&str> = bindings.iter().map(|b| b.as_str()).collect();
        self.document
            .add_layer(name, &refs)
            .map_err(LayoutEngineError::from)
    }

    /// Remove a layer by name.
    ///
    /// # Arguments
    /// * `name` - The name of the layer to remove
    ///
    /// # Returns
    /// * `Ok(())` if successful
    /// * `Err(LayoutEngineError)` if layer doesn't exist or cannot be removed
    pub fn remove_layer(&mut self, name: &str) -> Result<(), LayoutEngineError> {
        // Check if layer exists
        if !self.layer_names().contains(&name.to_string()) {
            return Err(LayoutEngineError::Validation(format!(
                "layer '{}' does not exist",
                name
            )));
        }

        // Remove the layer using the provider
        self.document
            .remove_layer(name)
            .map_err(LayoutEngineError::from)
    }

    /// Get information about a specific layer.
    ///
    /// # Arguments
    /// * `name` - The name of the layer
    ///
    /// # Returns
    /// * `Some(LayerInfo)` if the layer exists
    /// * `None` if the layer doesn't exist
    pub fn get_layer(&self, name: &str) -> Option<LayerInfo> {
        if !self.layer_names().contains(&name.to_string()) {
            return None;
        }

        let bindings = self.layer_bindings(name).ok()?;
        let index = self
            .layer_names()
            .iter()
            .position(|n| n == name)
            .unwrap_or(0);

        Some(LayerInfo {
            name: name.to_string(),
            index,
            binding_count: bindings.len(),
            bindings,
        })
    }

    /// List all layers with their information.
    ///
    /// # Returns
    /// * Vector of `LayerInfo` for all layers in the keymap
    pub fn list_layers(&self) -> Vec<LayerInfo> {
        let names = self.layer_names();
        names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                let bindings = self.layer_bindings(name).ok()?;
                Some(LayerInfo {
                    name: name.clone(),
                    index,
                    binding_count: bindings.len(),
                    bindings,
                })
            })
            .collect()
    }
}

/// Information about a layer in the keymap.
#[derive(Debug, Clone)]
pub struct LayerInfo {
    /// The name of the layer
    pub name: String,
    /// The index/position of the layer in the keymap
    pub index: usize,
    /// The number of bindings in the layer
    pub binding_count: usize,
    /// The actual bindings
    pub bindings: Vec<String>,
}

fn layer_to_string(document: &DtsDocument, layer: &str) -> Option<String> {
    let node = find_layer_node(&document.items, layer)?;
    let prop = find_bindings_property(node)?;
    let bindings = parse_binding_list(&prop.value.raw);
    if bindings.is_empty() {
        None
    } else {
        Some(bindings.join(" "))
    }
}

fn combo_to_string(document: &DtsDocument, combo: &str) -> Option<String> {
    let combos_root = find_layer_node(&document.items, "combos")?;
    let combo_node = find_child_node(combos_root, combo)?;
    let mut parts = Vec::new();
    if let Some(prop) = combo_node
        .properties
        .iter()
        .find(|prop| prop.name == "key-positions")
    {
        parts.push(format!("key-positions={}", prop.value.raw.trim()));
    }
    if let Some(prop) = combo_node
        .properties
        .iter()
        .find(|prop| prop.name == "bindings")
    {
        parts.push(format!("bindings={}", prop.value.raw.trim()));
    }
    if let Some(prop) = combo_node
        .properties
        .iter()
        .find(|prop| prop.name == "timeout-ms")
    {
        parts.push(format!("timeout-ms={}", prop.value.raw.trim()));
    }
    if let Some(prop) = combo_node
        .properties
        .iter()
        .find(|prop| prop.name == "layers")
    {
        parts.push(format!("layers={}", prop.value.raw.trim()));
    }
    let conditions = combo_condition_comments(combo_node);
    if !conditions.is_empty() {
        parts.push(format!("conditions={}", conditions.join(" && ")));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("combo:{}:{}", combo, parts.join(";")))
    }
}

fn layer_order_to_string(document: &DtsDocument) -> String {
    if let Some(keymap) = find_layer_node(&document.items, "keymap") {
        let mut names = Vec::new();
        for item in &keymap.children {
            if let DtItem::Node(node) = item {
                if find_bindings_property(node).is_some() {
                    names.push(node.name.clone());
                }
            }
        }
        names.join(",")
    } else {
        String::new()
    }
}

fn combo_condition_comments(node: &DtNode) -> Vec<String> {
    node.leading_comments
        .iter()
        .filter_map(|comment| extract_condition_comment(&comment.text))
        .collect()
}

fn extract_condition_comment(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with(COMBO_CONDITION_COMMENT_PREFIX) {
        return None;
    }
    let body = trimmed[COMBO_CONDITION_COMMENT_PREFIX.len()..].trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

fn find_layer_node<'a>(items: &'a [DtItem], name: &str) -> Option<&'a DtNode> {
    for item in items {
        match item {
            DtItem::Node(node) => {
                if node.name == name {
                    return Some(node);
                }
                if let Some(found) = find_layer_node(&node.children, name) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_child_node<'a>(parent: &'a DtNode, name: &str) -> Option<&'a DtNode> {
    for item in &parent.children {
        if let DtItem::Node(node) = item {
            if node.name == name {
                return Some(node);
            }
        }
    }
    None
}

fn behavior_to_string(document: &DtsDocument, behavior: &str) -> Option<String> {
    let node = find_behavior_node(&document.items, behavior)?;
    let mut parts = Vec::new();
    for prop in &node.properties {
        let value = prop.value.raw.trim();
        if value.is_empty() {
            parts.push(format!("{};", prop.name));
        } else {
            parts.push(format!("{}={}", prop.name, value));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("behavior:{}:{}", behavior, parts.join(";")))
    }
}

fn find_behavior_node<'a>(items: &'a [DtItem], behavior: &str) -> Option<&'a DtNode> {
    for item in items {
        if let DtItem::Node(node) = item {
            if node.name == "behaviors" || node.name == "macros" {
                if let Some(found) = node.children.iter().find_map(|child| match child {
                    DtItem::Node(child) if child.name == behavior => Some(child),
                    _ => None,
                }) {
                    return Some(found);
                }
            }
            if let Some(found) = find_behavior_node(&node.children, behavior) {
                return Some(found);
            }
        }
    }
    None
}

fn array_to_binding_strings(array: &[TomlValue]) -> Result<Vec<String>, LayoutEngineError> {
    let mut result = Vec::with_capacity(array.len());
    for value in array {
        match value {
            TomlValue::String(text) => {
                if text.trim().is_empty() {
                    return Err(LayoutEngineError::Validation(
                        "behavior binding entries cannot be empty".into(),
                    ));
                }
                result.push(text.clone());
            }
            _ => {
                return Err(LayoutEngineError::Validation(
                    "behavior bindings must be strings".into(),
                ));
            }
        }
    }
    Ok(result)
}

fn meta_to_string(document: &DtsDocument, key: &str) -> Option<String> {
    let node = find_meta_node(&document.items)?;
    node.properties
        .iter()
        .find(|prop| prop.name == key)
        .map(|prop| prop.value.raw.trim().to_string())
}

fn set_meta_property(document: &mut DtsDocument, key: &str, value: String) {
    let node = ensure_meta_node(document);
    let property = ensure_property(node, key);
    property.value.raw = value;
}

fn ensure_meta_node(document: &mut DtsDocument) -> &mut DtNode {
    let items = document.items_mut();
    if let Some(index) = items
        .iter()
        .position(|item| matches!(item, DtItem::Node(node) if node.name == "meta"))
    {
        match items.get_mut(index) {
            Some(DtItem::Node(node)) => return node,
            _ => unreachable!(),
        }
    }
    items.push(DtItem::Node(DtNode {
        name: "meta".to_string(),
        raw_name: String::new(),
        span: empty_span(),
        properties: Vec::new(),
        children: Vec::new(),
        leading_comments: Vec::new(),
        trailing_comments: Vec::new(),
    }));
    match items.last_mut() {
        Some(DtItem::Node(node)) => node,
        _ => unreachable!(),
    }
}

fn find_meta_node(items: &[DtItem]) -> Option<&DtNode> {
    items.iter().find_map(|item| match item {
        DtItem::Node(node) if node.name == "meta" => Some(node),
        _ => None,
    })
}

fn ensure_property<'a>(node: &'a mut DtNode, name: &str) -> &'a mut DtProperty {
    if let Some(index) = node.properties.iter().position(|prop| prop.name == name) {
        return node
            .properties
            .get_mut(index)
            .expect("property index valid");
    }
    node.properties.push(DtProperty {
        name: name.to_string(),
        raw_name: String::new(),
        value: DtValue {
            raw: String::new(),
            span: empty_span(),
        },
        span: empty_span(),
        leading_comments: Vec::new(),
        trailing_comment: None,
    });
    node.properties.last_mut().expect("inserted property")
}

fn empty_span() -> TokenSpan {
    TokenSpan::new(0, 0, 1, 1, 1, 1)
}

fn find_bindings_property(node: &DtNode) -> Option<&DtProperty> {
    node.properties.iter().find(|prop| prop.name == "bindings")
}

fn format_metadata_value(value: &TomlValue) -> String {
    match value {
        TomlValue::String(text) => format!("\"{}\"", escape_string(text)),
        TomlValue::Integer(num) => format!("< {} >", num),
        TomlValue::Float(num) => format!("< {} >", num),
        TomlValue::Boolean(flag) => {
            if *flag {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        TomlValue::Array(items) => format_metadata_array(items),
        TomlValue::Table(entries) => {
            let body = entries
                .iter()
                .map(|(key, value)| format!("{} = {}", key, format_metadata_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {} }}", body)
        }
        TomlValue::Datetime(dt) => format!("\"{}\"", dt),
    }
}

fn format_metadata_array(items: &[TomlValue]) -> String {
    if items.is_empty() {
        "< >".to_string()
    } else if items
        .iter()
        .all(|item| matches!(item, TomlValue::Integer(_) | TomlValue::Float(_)))
    {
        let values = items
            .iter()
            .map(|item| format_metadata_value(item))
            .collect::<Vec<_>>();
        format!("< {} >", values.join(" "))
    } else {
        items
            .iter()
            .map(|item| match item {
                TomlValue::String(text) => format!("\"{}\"", escape_string(text)),
                _ => format_metadata_value(item),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn escape_string(input: &str) -> String {
    input.replace('"', "\\\"")
}

fn parse_binding_list(raw: &str) -> Vec<String> {
    parse_binding_groups(raw)
}

fn parse_binding_groups(raw: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for ch in raw.chars() {
        match ch {
            '<' => {
                if depth == 0 {
                    current.clear();
                } else {
                    current.push(ch);
                }
                depth += 1;
            }
            '>' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        let trimmed = current.trim();
                        if !trimmed.is_empty() {
                            if trimmed.matches('&').count() > 1 {
                                groups.extend(split_binding_sequence(trimmed));
                            } else {
                                groups.push(trimmed.to_string());
                            }
                        }
                    } else {
                        current.push(ch);
                    }
                }
            }
            _ => {
                if depth > 0 {
                    current.push(ch);
                }
            }
        }
    }
    if groups.is_empty() {
        let trimmed = raw
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim_end_matches(';')
            .trim();
        if !trimmed.is_empty() {
            if trimmed.matches('&').count() > 1 {
                groups.extend(split_binding_sequence(trimmed));
            } else {
                groups.push(trimmed.to_string());
            }
        }
    }
    groups
}

fn split_binding_sequence(sequence: &str) -> Vec<String> {
    let mut bindings = Vec::new();
    let mut current = String::new();
    for token in sequence.split_whitespace() {
        if token.starts_with('&') {
            if !current.is_empty() {
                bindings.push(current.trim().to_string());
                current.clear();
            }
            current.push_str(token);
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(token);
        }
    }
    if !current.is_empty() {
        bindings.push(current.trim().to_string());
    }
    bindings
}
