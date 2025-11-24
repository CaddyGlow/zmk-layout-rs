use crate::{
    ast::{DtItem, DtNode},
    bindings::{BindingParser, LayoutBinding},
    dts::DtsDocument,
};
use std::collections::HashMap;

use super::{
    ProviderError,
    behaviors::BehaviorProvider,
    combos::{ComboDefinition, ComboProvider, combo_node_mut},
    format::{BindingFormat, format_bindings_raw, format_u32_list, parse_binding_list},
    util::{
        ensure_bindings_property, ensure_layer_node, ensure_property, find_bindings_property,
        find_bindings_property_mut, find_layer_node, find_layer_node_mut, is_behavior_root,
    },
};

/// Provider that exposes convenience APIs for editing keymap layers, combos, and behaviors.
pub struct KeymapProvider {
    document: DtsDocument,
    parser: BindingParser,
}

impl KeymapProvider {
    pub fn new(document: DtsDocument) -> Self {
        Self {
            document,
            parser: BindingParser::new(),
        }
    }

    pub fn document(&self) -> &DtsDocument {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut DtsDocument {
        &mut self.document
    }

    pub fn into_document(self) -> DtsDocument {
        self.document
    }

    pub fn layer_names(&self) -> Vec<String> {
        find_layer_node(&self.document.items, "keymap")
            .map(|keymap| {
                keymap
                    .children
                    .iter()
                    .filter_map(|item| match item {
                        crate::ast::DtItem::Node(node)
                            if find_bindings_property(node).is_some() =>
                        {
                            Some(node.name.clone())
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn bindings_for_layer(&self, layer: &str) -> Result<Vec<LayoutBinding>, ProviderError> {
        let node = find_layer_node(&self.document.items, layer)
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        let bindings_prop = find_bindings_property(node)
            .ok_or_else(|| ProviderError::BindingsMissing(layer.to_string()))?;
        let entries = parse_binding_list(&bindings_prop.value.raw);
        Ok(entries
            .into_iter()
            .map(|entry| self.parser.parse_with_behavior_rules(&entry))
            .collect())
    }

    pub fn set_binding(
        &mut self,
        layer: &str,
        index: usize,
        binding: &str,
    ) -> Result<(), ProviderError> {
        let format = BindingFormat::new(&self.parser);
        let binding_str = format.normalize_binding(binding)?;

        let node = find_layer_node_mut(&mut self.document.items, layer)
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        let bindings_prop = find_bindings_property_mut(node)
            .ok_or_else(|| ProviderError::BindingsMissing(layer.to_string()))?;

        let mut entries = parse_binding_list(&bindings_prop.value.raw);
        if index >= entries.len() {
            return Err(ProviderError::BindingIndex {
                index,
                len: entries.len(),
            });
        }
        entries[index] = binding_str;
        bindings_prop.value.raw = format_bindings_raw(&entries);
        Ok(())
    }

    pub fn set_layer_bindings(
        &mut self,
        layer: &str,
        bindings: &[&str],
    ) -> Result<(), ProviderError> {
        let format = BindingFormat::new(&self.parser);
        let normalized = format.normalize_bindings(bindings)?;
        ensure_layer_node(&mut self.document.items, layer)?;
        let node = find_layer_node_mut(&mut self.document.items, layer)
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        let bindings_prop = ensure_bindings_property(node);
        bindings_prop.value.raw = format_bindings_raw(&normalized);
        Ok(())
    }

    pub fn set_layer_metadata(
        &mut self,
        layer: &str,
        metadata: &[(String, String)],
    ) -> Result<(), ProviderError> {
        if metadata.is_empty() {
            return Ok(());
        }
        ensure_layer_node(&mut self.document.items, layer)?;
        let node = find_layer_node_mut(&mut self.document.items, layer)
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        for (key, value) in metadata {
            let property = ensure_property(node, key);
            property.value.raw = value.clone();
        }
        Ok(())
    }

    pub fn set_combo_bindings(
        &mut self,
        combo: &str,
        bindings: &[&str],
    ) -> Result<(), ProviderError> {
        let format = BindingFormat::new(&self.parser);
        let binding_values = format.normalize_bindings(bindings)?;
        let combo_node = combo_node_mut(&mut self.document.items, combo)?;
        let bindings_prop = find_bindings_property_mut(combo_node).ok_or_else(|| {
            ProviderError::PropertyMissing {
                property: "bindings".to_string(),
                context: combo.to_string(),
            }
        })?;
        bindings_prop.value.raw = format_bindings_raw(&binding_values);
        Ok(())
    }

    pub fn set_combo_key_positions(
        &mut self,
        combo: &str,
        positions: &[u32],
    ) -> Result<(), ProviderError> {
        if positions.is_empty() {
            return Err(ProviderError::InvalidBinding(
                "combo must declare at least one key position".to_string(),
            ));
        }
        let combo_node = combo_node_mut(&mut self.document.items, combo)?;
        let property = combo_node
            .properties
            .iter_mut()
            .find(|prop| prop.name == "key-positions")
            .ok_or_else(|| ProviderError::PropertyMissing {
                property: "key-positions".to_string(),
                context: combo.to_string(),
            })?;
        let values = positions
            .iter()
            .map(|pos| pos.to_string())
            .collect::<Vec<_>>();
        property.value.raw = super::format::format_list(&values);
        Ok(())
    }

    pub fn set_combo_timeout_ms(
        &mut self,
        combo: &str,
        timeout_ms: Option<u32>,
    ) -> Result<(), ProviderError> {
        let combo_node = combo_node_mut(&mut self.document.items, combo)?;
        if let Some(value) = timeout_ms {
            let property = ensure_property(combo_node, "timeout-ms");
            property.value.raw = format_list(&[value.to_string()]);
        } else {
            combo_node
                .properties
                .retain(|prop| prop.name != "timeout-ms");
        }
        Ok(())
    }

    pub fn set_combo_layers(&mut self, combo: &str, layers: &[u32]) -> Result<(), ProviderError> {
        let combo_node = combo_node_mut(&mut self.document.items, combo)?;
        if layers.is_empty() {
            combo_node.properties.retain(|prop| prop.name != "layers");
        } else {
            let property = ensure_property(combo_node, "layers");
            property.value.raw = format_u32_list(layers);
        }
        Ok(())
    }

    pub fn upsert_combo(
        &mut self,
        combo: &str,
        binding: &str,
        key_positions: &[u32],
        timeout_ms: Option<u32>,
        layers: &[u32],
        conditions: &[String],
    ) -> Result<(), ProviderError> {
        if key_positions.is_empty() {
            return Err(ProviderError::InvalidBinding(
                "combo must declare at least one key position".to_string(),
            ));
        }
        let format = BindingFormat::new(&self.parser);
        let normalized = format.normalize_bindings(&[binding])?;
        apply_combo_metadata(
            &mut self.document.items,
            combo,
            binding,
            key_positions,
            normalized,
            timeout_ms,
            layers,
            conditions,
        )
    }

    pub fn move_layer_to_index(
        &mut self,
        layer: &str,
        position: usize,
    ) -> Result<(), ProviderError> {
        let keymap_root = find_layer_node_mut(&mut self.document.items, "keymap")
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        let current_index = keymap_root
            .children
            .iter()
            .position(|item| matches!(item, crate::ast::DtItem::Node(node) if node.name == layer))
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;

        let max_index = keymap_root.children.len();
        let target_index = position.min(max_index);
        if current_index == target_index {
            return Ok(());
        }

        let item = keymap_root.children.remove(current_index);
        let adjusted = if target_index > current_index {
            target_index.saturating_sub(1)
        } else {
            target_index
        };
        let insert_at = adjusted.min(keymap_root.children.len());
        keymap_root.children.insert(insert_at, item);
        Ok(())
    }

    pub fn set_behavior_bindings(
        &mut self,
        behavior: &str,
        bindings: &[&str],
    ) -> Result<(), ProviderError> {
        let format = BindingFormat::new(&self.parser);
        let binding_values = format.normalize_bindings(bindings)?;
        let behavior_node = self.behavior_node_mut(behavior)?;
        let property = find_bindings_property_mut(behavior_node).ok_or_else(|| {
            ProviderError::PropertyMissing {
                property: "bindings".to_string(),
                context: behavior.to_string(),
            }
        })?;
        property.value.raw = format_bindings_raw(&binding_values);
        Ok(())
    }

    pub fn set_behavior_properties(
        &mut self,
        behavior: &str,
        properties: &[(String, String)],
    ) -> Result<(), ProviderError> {
        if properties.is_empty() {
            return Ok(());
        }
        let behavior_node = self.behavior_node_mut(behavior)?;
        for (key, value) in properties {
            let property = ensure_property(behavior_node, key);
            property.value.raw = value.clone();
        }
        Ok(())
    }

    pub fn set_macro_timing(
        &mut self,
        behavior: &str,
        wait_ms: Option<u32>,
        tap_ms: Option<u32>,
    ) -> Result<(), ProviderError> {
        self.set_behavior_numeric_property(behavior, "wait-ms", wait_ms)?;
        self.set_behavior_numeric_property(behavior, "tap-ms", tap_ms)
    }

    pub fn set_behavior_binding_cells(
        &mut self,
        behavior: &str,
        binding_cells: Option<u32>,
    ) -> Result<(), ProviderError> {
        self.set_behavior_numeric_property(behavior, "#binding-cells", binding_cells)
    }

    pub fn set_behavior_label(
        &mut self,
        behavior: &str,
        label: Option<&str>,
    ) -> Result<(), ProviderError> {
        let behavior_node = self.behavior_node_mut(behavior)?;
        if let Some(text) = label {
            let property = ensure_property(behavior_node, "label");
            property.value.raw = format!("\"{}\"", text);
        } else {
            behavior_node.properties.retain(|prop| prop.name != "label");
        }
        Ok(())
    }

    fn set_behavior_numeric_property(
        &mut self,
        behavior: &str,
        property: &str,
        value: Option<u32>,
    ) -> Result<(), ProviderError> {
        let behavior_node = self.behavior_node_mut(behavior)?;
        if let Some(value) = value {
            let prop = ensure_property(behavior_node, property);
            prop.value.raw = format_u32_list(&[value]);
        } else {
            behavior_node
                .properties
                .retain(|prop| prop.name != property);
        }
        Ok(())
    }

    fn behavior_node_mut(&mut self, behavior: &str) -> Result<&mut DtNode, ProviderError> {
        let root_index = ensure_behaviors_root(&mut self.document.items);
        let root = match self.document.items.get_mut(root_index) {
            Some(crate::ast::DtItem::Node(node)) => node,
            _ => unreachable!(),
        };
        if let Some(idx) = root.children.iter().position(
            |item| matches!(item, crate::ast::DtItem::Node(node) if node.name == behavior),
        ) {
            match root.children.get_mut(idx) {
                Some(crate::ast::DtItem::Node(node)) => return Ok(node),
                _ => unreachable!(),
            }
        }
        root.children
            .push(crate::ast::DtItem::Node(super::util::empty_node(behavior)));
        match root.children.last_mut() {
            Some(crate::ast::DtItem::Node(node)) => Ok(node),
            _ => unreachable!(),
        }
    }

    fn ensure_layer_node(&mut self, layer: &str) -> Result<(), ProviderError> {
        ensure_layer_node(&mut self.document.items, layer)
    }
}

fn ensure_behaviors_root(document_items: &mut Vec<crate::ast::DtItem>) -> usize {
    if let Some(index) = document_items
        .iter()
        .position(|item| matches!(item, crate::ast::DtItem::Node(node) if is_behavior_root(node)))
    {
        return index;
    }
    document_items.push(crate::ast::DtItem::Node(super::util::empty_node(
        "behaviors",
    )));
    document_items.len() - 1
}

/// High-level keymap document that reuses the provider stack.
#[derive(Debug, Clone)]
pub struct KeymapDocument {
    document: DtsDocument,
}

impl KeymapDocument {
    pub fn parse_str(source: &str) -> Result<Self, crate::tokenizer::LayoutError> {
        let document = DtsDocument::parse_str(source)?;
        Ok(Self { document })
    }

    pub fn from_document(document: DtsDocument) -> Self {
        Self { document }
    }

    pub fn document(&self) -> &DtsDocument {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut DtsDocument {
        &mut self.document
    }

    pub fn into_document(self) -> DtsDocument {
        self.document
    }

    pub fn layer_names(&self) -> Vec<String> {
        KeymapProvider::new(self.document.clone()).layer_names()
    }

    pub fn behaviors(&self) -> Vec<super::behaviors::BehaviorDefinition> {
        BehaviorProvider::new(&self.document).behaviors()
    }

    pub fn combos(&self) -> Vec<ComboDefinition> {
        ComboProvider::new(&self.document).combos()
    }

    pub fn bindings_for_layer(&self, layer: &str) -> Result<Vec<LayoutBinding>, ProviderError> {
        KeymapProvider::new(self.document.clone()).bindings_for_layer(layer)
    }

    pub fn set_binding(
        &mut self,
        layer: &str,
        index: usize,
        binding: &str,
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_binding(layer, index, binding)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_behavior_properties(
        &mut self,
        behavior: &str,
        properties: &[(String, String)],
    ) -> Result<(), ProviderError> {
        if properties.is_empty() {
            return Ok(());
        }
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_behavior_properties(behavior, properties)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_macro_timing(
        &mut self,
        behavior: &str,
        wait_ms: Option<u32>,
        tap_ms: Option<u32>,
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_macro_timing(behavior, wait_ms, tap_ms)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_behavior_binding_cells(
        &mut self,
        behavior: &str,
        binding_cells: Option<u32>,
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_behavior_binding_cells(behavior, binding_cells)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_behavior_bindings(
        &mut self,
        behavior: &str,
        bindings: &[&str],
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_behavior_bindings(behavior, bindings)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_behavior_label(
        &mut self,
        behavior: &str,
        label: Option<&str>,
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_behavior_label(behavior, label)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_layer_bindings(
        &mut self,
        layer: &str,
        bindings: &[&str],
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_layer_bindings(layer, bindings)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_layer_metadata(
        &mut self,
        layer: &str,
        metadata: &[(String, String)],
    ) -> Result<(), ProviderError> {
        if metadata.is_empty() {
            return Ok(());
        }
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_layer_metadata(layer, metadata)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_combo_bindings(
        &mut self,
        combo: &str,
        bindings: &[&str],
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_combo_bindings(combo, bindings)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_combo_key_positions(
        &mut self,
        combo: &str,
        positions: &[u32],
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_combo_key_positions(combo, positions)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_combo_timeout_ms(
        &mut self,
        combo: &str,
        timeout_ms: Option<u32>,
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_combo_timeout_ms(combo, timeout_ms)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn set_combo_layers(&mut self, combo: &str, layers: &[u32]) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_combo_layers(combo, layers)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn upsert_combo(
        &mut self,
        name: &str,
        binding: &str,
        key_positions: &[u32],
        timeout_ms: Option<u32>,
        layers: &[u32],
        conditions: &[String],
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.upsert_combo(name, binding, key_positions, timeout_ms, layers, conditions)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn move_layer_to_index(
        &mut self,
        layer: &str,
        position: usize,
    ) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.move_layer_to_index(layer, position)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn reorder_layer(&mut self, layer: &str, position: usize) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.move_layer_to_index(layer, position)?;
        self.document = provider.into_document();

        self.update_layer_defines()?;
        Ok(())
    }

    pub fn add_layer(&mut self, name: &str, bindings: &[&str]) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.ensure_layer_node(name)?;
        provider.set_layer_bindings(name, bindings)?;
        self.document = provider.into_document();

        self.update_layer_defines()?;
        Ok(())
    }

    pub fn remove_layer(&mut self, name: &str) -> Result<(), ProviderError> {
        let mut document = self.document.clone();
        if let Some(keymap) = find_layer_node_mut(&mut document.items, "keymap") {
            keymap
                .children
                .retain(|item| !matches!(item, DtItem::Node(node) if node.name == name));
            self.document = document;
            self.update_layer_defines()?;
            return Ok(());
        }
        Err(ProviderError::LayerNotFound(name.to_string()))
    }
}

fn format_list(values: &[String]) -> String {
    super::format::format_list(values)
}

fn apply_combo_metadata(
    document_items: &mut Vec<crate::ast::DtItem>,
    combo: &str,
    binding: &str,
    key_positions: &[u32],
    normalized: Vec<String>,
    timeout_ms: Option<u32>,
    layers: &[u32],
    conditions: &[String],
) -> Result<(), ProviderError> {
    super::combos::apply_combo_metadata(
        document_items,
        combo,
        binding,
        key_positions,
        normalized,
        timeout_ms,
        layers,
        conditions,
    )
}

impl KeymapDocument {
    fn update_layer_defines(&mut self) -> Result<(), ProviderError> {
        // Get current layer order
        let layer_names = self.layer_names();

        // Build a map of canonical layer names to indices so we can match
        // macros regardless of case or sanitization differences.
        let mut layer_indices = HashMap::new();
        for (idx, name) in layer_names.iter().enumerate() {
            layer_indices.insert(Self::canonical_layer_define_key(name), idx);
        }

        // Update all matching macros
        for item in &mut self.document.items {
            if let DtItem::Macro(mac) = item {
                if let Some(parsed) = Self::parse_layer_define(&mac.text) {
                    let canonical = Self::canonical_layer_define_key(&parsed.name);
                    if let Some(&new_idx) = layer_indices.get(&canonical) {
                        let mut updated = format!(
                            "{}#define LAYER_{} {}",
                            parsed.leading, parsed.name, new_idx
                        );
                        if let Some(comment) = parsed.comment {
                            if comment
                                .chars()
                                .next()
                                .map_or(false, |ch| ch.is_whitespace())
                            {
                                updated.push_str(&comment);
                            } else {
                                updated.push(' ');
                                updated.push_str(&comment);
                            }
                        }
                        mac.text = updated;
                    }
                }
            }
        }

        Ok(())
    }

    fn parse_layer_define(text: &str) -> Option<LayerDefineLine> {
        let (code, comment) = Self::split_layer_define_comment(text);
        if code.is_empty() {
            return None;
        }
        let trimmed_start = code.trim_start_matches(|c| matches!(c, ' ' | '\t'));
        let leading_len = code.len() - trimmed_start.len();
        let leading = code[..leading_len].to_string();
        let mut parts = trimmed_start.split_whitespace();
        if parts.next()? != "#define" {
            return None;
        }
        let identifier = parts.next()?;
        if !identifier.starts_with("LAYER_") {
            return None;
        }
        // Ensure a value exists but we don't care what it is.
        parts.next()?;
        Some(LayerDefineLine {
            leading,
            name: identifier["LAYER_".len()..].to_string(),
            comment: comment.map(|value| value.to_string()),
        })
    }

    fn sanitize_layer_define_name(name: &str) -> String {
        let trimmed = if name.len() >= 6 && name[..6].eq_ignore_ascii_case("layer_") {
            &name[6..]
        } else {
            name
        };
        let mut result = String::new();
        for ch in trimmed.chars() {
            if ch.is_ascii_alphanumeric() {
                result.push(ch);
                continue;
            }
            result.push('_');
        }
        if result.is_empty() {
            "LAYER".to_string()
        } else {
            result
        }
    }

    fn canonical_layer_define_key(name: &str) -> String {
        Self::sanitize_layer_define_name(name).to_ascii_lowercase()
    }

    fn split_layer_define_comment(text: &str) -> (&str, Option<&str>) {
        let mut chars = text.char_indices().peekable();
        while let Some((idx, ch)) = chars.next() {
            match ch {
                '"' | '\'' => {
                    let quote = ch;
                    let mut escaped = false;
                    while let Some((_, next)) = chars.next() {
                        if escaped {
                            escaped = false;
                            continue;
                        }
                        if next == '\\' {
                            escaped = true;
                            continue;
                        }
                        if next == quote {
                            break;
                        }
                    }
                }
                '/' => {
                    if let Some((_, next)) = chars.peek().copied() {
                        if next == '/' || next == '*' {
                            let prefix = &text[..idx];
                            let trimmed = Self::trim_trailing_inline_ws(prefix);
                            let comment = &text[trimmed.len()..];
                            return (trimmed, Some(comment));
                        }
                    }
                }
                _ => {}
            }
        }
        (Self::trim_trailing_inline_ws(text), None)
    }

    fn trim_trailing_inline_ws(text: &str) -> &str {
        text.trim_end_matches(|c| matches!(c, ' ' | '\t' | '\r'))
    }
}

#[derive(Debug)]
struct LayerDefineLine {
    leading: String,
    name: String,
    comment: Option<String>,
}
