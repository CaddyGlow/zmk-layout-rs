use crate::{
    ast::{DtComment, DtItem, DtNode},
    bindings::{BindingParser, LayoutBinding},
    dts::DtsDocument,
};

use super::{
    format::{parse_binding_groups, parse_numeric_list, parse_numeric_value},
    util::{apply_combo_conditions, ensure_property, find_child_node_mut, find_layer_node_mut},
    ProviderError,
};

/// Provider that enumerates combos and exposes their metadata.
pub struct ComboProvider<'a> {
    document: &'a DtsDocument,
    parser: BindingParser,
}

impl<'a> ComboProvider<'a> {
    pub fn new(document: &'a DtsDocument) -> Self {
        Self {
            document,
            parser: BindingParser::new(),
        }
    }

    pub fn combos(&self) -> Vec<ComboDefinition> {
        let mut result = Vec::new();
        for item in &self.document.items {
            collect_combos(item, &self.parser, &mut result);
        }
        result
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComboDefinition {
    pub name: String,
    pub key_positions: Vec<u32>,
    pub timeout_ms: Option<u32>,
    pub bindings: Vec<LayoutBinding>,
    pub layers: Vec<u32>,
    pub description: Option<String>,
    pub conditions: Vec<String>,
    pub properties: Vec<NodeProperty>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProperty {
    pub name: String,
    pub raw_value: Option<String>,
}

fn collect_combos(item: &DtItem, parser: &BindingParser, acc: &mut Vec<ComboDefinition>) {
    match item {
        DtItem::Node(node) => {
            if node
                .properties
                .iter()
                .any(|prop| prop.name == "key-positions")
            {
                let key_positions = parse_key_positions(node);
                let timeout_ms = node
                    .properties
                    .iter()
                    .find(|prop| prop.name == "timeout-ms")
                    .and_then(|prop| parse_numeric_value(&prop.value.raw));
                let bindings = binding_list(node)
                    .into_iter()
                    .map(|raw| parser.parse_with_behavior_rules(&raw))
                    .collect();
                let layers = parse_layers(node);
                let description = combo_description(node);
                let conditions = super::util::combo_condition_comments(&node.leading_comments);
                acc.push(ComboDefinition {
                    name: node.name.clone(),
                    key_positions,
                    timeout_ms,
                    bindings,
                    layers,
                    description,
                    conditions,
                    properties: capture_node_properties(node),
                });
            }
            for child in &node.children {
                collect_combos(child, parser, acc);
            }
        }
        _ => {}
    }
}

fn parse_key_positions(node: &DtNode) -> Vec<u32> {
    node.properties
        .iter()
        .find(|prop| prop.name == "key-positions")
        .map(|prop| {
            prop.value
                .raw
                .trim_matches(['<', '>', ';', ' '])
                .split_whitespace()
                .filter_map(|v| v.parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn parse_layers(node: &DtNode) -> Vec<u32> {
    node.properties
        .iter()
        .find(|prop| prop.name == "layers")
        .map(|prop| parse_numeric_list(&prop.value.raw))
        .unwrap_or_default()
}

fn binding_list(node: &DtNode) -> Vec<String> {
    node.properties
        .iter()
        .find(|prop| prop.name == "bindings")
        .map(|prop| parse_binding_groups(&prop.value.raw))
        .unwrap_or_default()
}

fn combo_description(node: &DtNode) -> Option<String> {
    if let Some(prop) = node
        .properties
        .iter()
        .find(|prop| prop.name == "description")
    {
        return Some(trim_string_literal(&prop.value.raw));
    }
    extract_comment_text(&node.leading_comments)
}

fn trim_string_literal(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed.trim_matches('"').trim().to_string()
}

fn extract_comment_text(comments: &[DtComment]) -> Option<String> {
    collect_trailing_comment_lines(comments).map(|lines| lines.join("\n"))
}

fn collect_trailing_comment_lines(comments: &[DtComment]) -> Option<Vec<String>> {
    let mut lines = Vec::new();
    let mut started = false;
    for comment in comments.iter().rev() {
        match normalize_comment_text(&comment.text) {
            Some(text) => {
                started = true;
                lines.push(text);
            }
            None => {
                if started {
                    lines.push(String::new());
                }
            }
        }
    }
    if !started {
        return None;
    }
    lines.reverse();
    let start = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .unwrap_or(start);
    let slice = lines[start..=end].to_vec();
    Some(slice)
}

fn normalize_comment_text(text: &str) -> Option<String> {
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

pub(crate) fn capture_node_properties(node: &DtNode) -> Vec<NodeProperty> {
    node.properties
        .iter()
        .map(|prop| NodeProperty {
            name: prop.name.clone(),
            raw_value: normalize_behavior_property_value(&prop.value.raw),
        })
        .collect()
}

fn normalize_behavior_property_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.trim_end_matches(';').trim().to_string())
    }
}

pub fn ensure_combos_root_index(document_items: &mut Vec<DtItem>) -> usize {
    if let Some(index) = document_items
        .iter()
        .position(|item| matches!(item, DtItem::Node(node) if node.name == "combos"))
    {
        return index;
    }
    document_items.push(DtItem::Node(super::util::empty_node("combos")));
    document_items.len() - 1
}

pub fn combo_node_mut<'a>(
    document_items: &'a mut [DtItem],
    combo: &str,
) -> Result<&'a mut DtNode, ProviderError> {
    let combos_root =
        find_layer_node_mut(document_items, "combos").ok_or(ProviderError::CombosMissing)?;
    find_child_node_mut(combos_root, combo)
        .ok_or_else(|| ProviderError::ComboNotFound(combo.to_string()))
}

#[allow(dead_code)]
pub fn apply_combo_conditions_to_node(node: &mut DtNode, conditions: &[String]) {
    apply_combo_conditions(node, conditions);
}

pub fn binding_cells_value(node: &DtNode) -> Option<u32> {
    node.properties
        .iter()
        .find(|prop| prop.name == "#binding-cells")
        .and_then(|prop| {
            prop.value
                .raw
                .trim_matches(['<', '>', ' ', ';'])
                .parse()
                .ok()
        })
}

pub fn binding_list_from_node(node: &DtNode) -> Vec<String> {
    node.properties
        .iter()
        .find(|prop| prop.name == "bindings")
        .map(|prop| parse_binding_groups(&prop.value.raw))
        .unwrap_or_default()
}

pub fn behavior_description(node: &DtNode) -> Option<String> {
    extract_comment_text(&node.leading_comments)
}

pub fn parse_optional_numeric_property(node: &DtNode, name: &str) -> Option<u32> {
    node.properties
        .iter()
        .find(|prop| prop.name == name)
        .and_then(|prop| parse_numeric_value(&prop.value.raw))
}

pub fn compatible_value(node: &DtNode) -> Option<String> {
    node.properties
        .iter()
        .find(|prop| prop.name == "compatible")
        .map(|prop| trim_string_literal(&prop.value.raw))
}

pub fn apply_combo_metadata(
    document_items: &mut Vec<DtItem>,
    combo: &str,
    _binding: &str,
    key_positions: &[u32],
    normalized: Vec<String>,
    timeout_ms: Option<u32>,
    layers: &[u32],
    conditions: &[String],
) -> Result<(), ProviderError> {
    let combos_index = ensure_combos_root_index(document_items);
    let combo_node = match document_items.get_mut(combos_index) {
        Some(DtItem::Node(node)) => {
            let child_index = node
                .children
                .iter()
                .position(|item| matches!(item, DtItem::Node(child) if child.name == combo));
            let target_node = if let Some(idx) = child_index {
                match node.children.get_mut(idx) {
                    Some(DtItem::Node(child)) => child,
                    _ => unreachable!(),
                }
            } else {
                node.children
                    .push(DtItem::Node(super::util::empty_node(combo)));
                match node.children.last_mut() {
                    Some(DtItem::Node(child)) => child,
                    _ => unreachable!(),
                }
            };
            target_node
        }
        _ => unreachable!(),
    };

    let key_prop = ensure_property(combo_node, "key-positions");
    key_prop.value.raw = super::format::format_u32_list(key_positions);

    let bindings_prop = ensure_property(combo_node, "bindings");
    bindings_prop.value.raw = super::format::format_bindings_raw(&normalized);

    if let Some(value) = timeout_ms {
        let timeout_prop = ensure_property(combo_node, "timeout-ms");
        timeout_prop.value.raw = super::format::format_u32_list(&[value]);
    } else {
        combo_node
            .properties
            .retain(|prop| prop.name != "timeout-ms");
    }

    if layers.is_empty() {
        combo_node.properties.retain(|prop| prop.name != "layers");
    } else {
        let layer_prop = ensure_property(combo_node, "layers");
        layer_prop.value.raw = super::format::format_u32_list(layers);
    }

    apply_combo_conditions(combo_node, conditions);
    Ok(())
}
