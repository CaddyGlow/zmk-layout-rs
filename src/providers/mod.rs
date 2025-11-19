//! High-level helpers for mutating keymap documents.

use thiserror::Error;

use crate::{
    ast::{DtComment, DtItem, DtNode, DtProperty, DtValue},
    bindings::{BindingParser, LayoutBinding},
    dts::DtsDocument,
    tokenizer::TokenSpan,
};

pub(crate) const COMBO_CONDITION_COMMENT_PREFIX: &str = "// zmk-task:condition";

/// Provider that exposes convenience APIs for editing keymap layers.
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
                        DtItem::Node(node) if find_bindings_property(node).is_some() => {
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
        let binding_str = self.normalize_binding(binding)?;

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
        let normalized = self.normalize_bindings(bindings)?;
        self.ensure_layer_node(layer)?;
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
        self.ensure_layer_node(layer)?;
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
        let binding_values = self.normalize_bindings(bindings)?;
        let combo_node = self.combo_node_mut(combo)?;
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
        let combo_node = self.combo_node_mut(combo)?;
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
        property.value.raw = format_list(&values);
        Ok(())
    }

    pub fn set_combo_timeout_ms(
        &mut self,
        combo: &str,
        timeout_ms: Option<u32>,
    ) -> Result<(), ProviderError> {
        let combo_node = self.combo_node_mut(combo)?;
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
        let combo_node = self.combo_node_mut(combo)?;
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
        let normalized = self.normalize_bindings(&[binding])?;
        let combos_index = self.ensure_combos_root_index();
        let combo_node = match self.document.items.get_mut(combos_index) {
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
                    node.children.push(DtItem::Node(DtNode {
                        name: combo.to_string(),
                        raw_name: String::new(),
                        span: empty_span(),
                        properties: Vec::new(),
                        children: Vec::new(),
                        leading_comments: Vec::new(),
                        trailing_comments: Vec::new(),
                    }));
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
        key_prop.value.raw = format_u32_list(key_positions);

        let bindings_prop = ensure_property(combo_node, "bindings");
        bindings_prop.value.raw = format_bindings_raw(&normalized);

        if let Some(value) = timeout_ms {
            let timeout_prop = ensure_property(combo_node, "timeout-ms");
            timeout_prop.value.raw = format_u32_list(&[value]);
        } else {
            combo_node
                .properties
                .retain(|prop| prop.name != "timeout-ms");
        }

        if layers.is_empty() {
            combo_node.properties.retain(|prop| prop.name != "layers");
        } else {
            let layer_prop = ensure_property(combo_node, "layers");
            layer_prop.value.raw = format_u32_list(layers);
        }

        crate::providers::apply_combo_conditions(combo_node, conditions);
        Ok(())
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
            .position(|item| matches!(item, DtItem::Node(node) if node.name == layer))
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
        let binding_values = self.normalize_bindings(bindings)?;
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

    fn normalize_binding(&self, binding: &str) -> Result<String, ProviderError> {
        if binding.trim().is_empty() {
            return Err(ProviderError::InvalidBinding(
                "binding string cannot be empty".to_string(),
            ));
        }
        Ok(self
            .parser
            .parse_with_behavior_rules(binding)
            .to_binding_string())
    }

    fn normalize_bindings(&self, bindings: &[&str]) -> Result<Vec<String>, ProviderError> {
        if bindings.is_empty() {
            return Err(ProviderError::InvalidBinding(
                "at least one binding is required".to_string(),
            ));
        }
        bindings
            .iter()
            .map(|binding| self.normalize_binding(binding))
            .collect()
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

    fn combo_node_mut(&mut self, combo: &str) -> Result<&mut DtNode, ProviderError> {
        let combos_root = find_layer_node_mut(&mut self.document.items, "combos")
            .ok_or(ProviderError::CombosMissing)?;
        find_child_node_mut(combos_root, combo)
            .ok_or_else(|| ProviderError::ComboNotFound(combo.to_string()))
    }

    fn ensure_combos_root_index(&mut self) -> usize {
        if let Some(index) = self
            .document
            .items
            .iter()
            .position(|item| matches!(item, DtItem::Node(node) if node.name == "combos"))
        {
            return index;
        }
        self.document.items.push(DtItem::Node(DtNode {
            name: "combos".to_string(),
            raw_name: String::new(),
            span: empty_span(),
            properties: Vec::new(),
            children: Vec::new(),
            leading_comments: Vec::new(),
            trailing_comments: Vec::new(),
        }));
        self.document.items.len() - 1
    }

    fn behavior_node_mut(&mut self, behavior: &str) -> Result<&mut DtNode, ProviderError> {
        let mut root_found = false;
        for item in &mut self.document.items {
            if let DtItem::Node(node) = item {
                if node.name == "behaviors" || node.name == "macros" {
                    root_found = true;
                    if let Some(child) = find_child_node_mut(node, behavior) {
                        return Ok(child);
                    }
                }
            }
        }
        if !root_found {
            return Err(ProviderError::BehaviorsMissing);
        }
        Err(ProviderError::BehaviorNotFound(behavior.to_string()))
    }

    fn ensure_layer_node(&mut self, layer: &str) -> Result<(), ProviderError> {
        let keymap_root = find_layer_node_mut(&mut self.document.items, "keymap")
            .ok_or_else(|| ProviderError::LayerNotFound(layer.to_string()))?;
        if let Some(idx) = keymap_root
            .children
            .iter()
            .position(|item| matches!(item, DtItem::Node(node) if node.name == layer))
        {
            if let Some(DtItem::Node(_)) = keymap_root.children.get_mut(idx) {
                return Ok(());
            }
        }

        // HACK: some templates rely on macros (e.g., ZMK_DEFINE_LAYER) to generate the
        // actual layer nodes, so we synthesize a placeholder here to keep the adapter happy.
        let span = empty_span();
        let new_node = DtNode {
            name: layer.to_string(),
            raw_name: String::new(),
            span,
            properties: vec![DtProperty {
                name: "bindings".to_string(),
                raw_name: String::new(),
                value: DtValue {
                    raw: "< >".to_string(),
                    span: empty_span(),
                },
                span: empty_span(),
                leading_comments: Vec::new(),
                trailing_comment: None,
            }],
            children: Vec::new(),
            leading_comments: Vec::new(),
            trailing_comments: Vec::new(),
        };
        keymap_root.children.push(DtItem::Node(new_node));
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("layer `{0}` not found")]
    LayerNotFound(String),
    #[error("bindings property missing for layer `{0}`")]
    BindingsMissing(String),
    #[error("combos section missing from document")]
    CombosMissing,
    #[error("combo `{0}` not found")]
    ComboNotFound(String),
    #[error("behaviors section missing from document")]
    BehaviorsMissing,
    #[error("behavior `{0}` not found")]
    BehaviorNotFound(String),
    #[error("property `{property}` missing for `{context}`")]
    PropertyMissing { property: String, context: String },
    #[error("{0}")]
    InvalidBinding(String),
    #[error("binding index {index} out of range (len {len})")]
    BindingIndex { index: usize, len: usize },
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

fn find_layer_node_mut<'a>(items: &'a mut [DtItem], name: &str) -> Option<&'a mut DtNode> {
    for item in items {
        match item {
            DtItem::Node(node) => {
                if node.name == name {
                    return Some(node);
                }
                if let Some(found) = find_layer_node_mut(&mut node.children, name) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_bindings_property(node: &DtNode) -> Option<&DtProperty> {
    node.properties.iter().find(|prop| prop.name == "bindings")
}

fn find_bindings_property_mut(node: &mut DtNode) -> Option<&mut DtProperty> {
    node.properties
        .iter_mut()
        .find(|prop| prop.name == "bindings")
}

fn ensure_bindings_property(node: &mut DtNode) -> &mut DtProperty {
    if let Some(idx) = node
        .properties
        .iter()
        .position(|prop| prop.name == "bindings")
    {
        return node
            .properties
            .get_mut(idx)
            .expect("bindings property index should be valid");
    }
    ensure_property(node, "bindings")
}

fn find_child_node_mut<'a>(parent: &'a mut DtNode, name: &str) -> Option<&'a mut DtNode> {
    parent.children.iter_mut().find_map(|item| match item {
        DtItem::Node(node) if node.name == name => Some(node),
        _ => None,
    })
}

fn ensure_property<'a>(node: &'a mut DtNode, name: &str) -> &'a mut DtProperty {
    if let Some(idx) = node.properties.iter().position(|prop| prop.name == name) {
        return node
            .properties
            .get_mut(idx)
            .expect("property index should be valid");
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
    node.properties
        .last_mut()
        .expect("just inserted property is present")
}

fn apply_combo_conditions(node: &mut DtNode, conditions: &[String]) {
    node.leading_comments
        .retain(|comment| !self::is_condition_comment(comment));
    if conditions.is_empty() {
        return;
    }
    for condition in conditions {
        let content = condition.trim();
        if content.is_empty() {
            continue;
        }
        let text = format!("{} {}", COMBO_CONDITION_COMMENT_PREFIX, content);
        node.leading_comments
            .push(DtComment::new(text, empty_span()));
    }
}

fn is_condition_comment(comment: &DtComment) -> bool {
    comment
        .text
        .trim_start()
        .starts_with(COMBO_CONDITION_COMMENT_PREFIX)
}

fn empty_span() -> TokenSpan {
    TokenSpan::new(0, 0, 1, 1, 1, 1)
}

fn parse_binding_groups(raw: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut depth = 0usize;
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

fn parse_binding_list(raw: &str) -> Vec<String> {
    parse_binding_groups(raw)
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

fn format_bindings_raw(bindings: &[String]) -> String {
    format_list(bindings)
}

fn format_list(values: &[String]) -> String {
    if values.is_empty() {
        "< >".to_string()
    } else {
        format!("< {} >", values.join(" "))
    }
}

fn format_u32_list(values: &[u32]) -> String {
    if values.is_empty() {
        "< >".to_string()
    } else {
        format!(
            "<{}>",
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

/// Read-only provider that lists behavior definitions.
pub struct BehaviorProvider<'a> {
    document: &'a DtsDocument,
}

impl<'a> BehaviorProvider<'a> {
    pub fn new(document: &'a DtsDocument) -> Self {
        Self { document }
    }

    pub fn behaviors(&self) -> Vec<BehaviorDefinition> {
        let mut results = Vec::new();
        for item in &self.document.items {
            collect_behaviors(item, &mut results);
        }
        results
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviorDefinition {
    pub name: String,
    pub compatible: Option<String>,
    pub binding_cells: Option<u32>,
    pub bindings: Vec<String>,
    pub description: Option<String>,
    pub wait_ms: Option<u32>,
    pub tap_ms: Option<u32>,
    pub label: Option<String>,
    pub properties: Vec<NodeProperty>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProperty {
    pub name: String,
    pub raw_value: Option<String>,
}

fn collect_behaviors(item: &DtItem, acc: &mut Vec<BehaviorDefinition>) {
    match item {
        DtItem::Node(node) => {
            if is_behavior_node(node) {
                acc.push(BehaviorDefinition {
                    name: node.name.clone(),
                    compatible: compatible_value(node),
                    binding_cells: binding_cells_value(node),
                    bindings: binding_list(node),
                    description: behavior_description(node),
                    wait_ms: parse_optional_numeric_property(node, "wait-ms"),
                    tap_ms: parse_optional_numeric_property(node, "tap-ms"),
                    label: label_value(node),
                    properties: capture_node_properties(node),
                });
            }
            for child in &node.children {
                collect_behaviors(child, acc);
            }
        }
        _ => {}
    }
}

fn capture_node_properties(node: &DtNode) -> Vec<NodeProperty> {
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

fn is_behavior_node(node: &DtNode) -> bool {
    node.properties.iter().any(|prop| {
        prop.name == "compatible" && prop.value.raw.to_lowercase().contains("behavior-")
    })
}

fn compatible_value(node: &DtNode) -> Option<String> {
    node.properties
        .iter()
        .find(|prop| prop.name == "compatible")
        .map(|prop| trim_string_literal(&prop.value.raw))
}

fn binding_cells_value(node: &DtNode) -> Option<u32> {
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

fn binding_list(node: &DtNode) -> Vec<String> {
    node.properties
        .iter()
        .find(|prop| prop.name == "bindings")
        .map(|prop| parse_binding_groups(&prop.value.raw))
        .unwrap_or_default()
}

fn behavior_description(node: &DtNode) -> Option<String> {
    extract_comment_text(&node.leading_comments)
}

fn parse_optional_numeric_property(node: &DtNode, name: &str) -> Option<u32> {
    node.properties
        .iter()
        .find(|prop| prop.name == name)
        .and_then(|prop| parse_numeric_value(&prop.value.raw))
}

fn label_value(node: &DtNode) -> Option<String> {
    node.properties
        .iter()
        .find(|prop| prop.name == "label")
        .map(|prop| trim_string_literal(&prop.value.raw))
}

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
    pub properties: Vec<NodeProperty>,
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
                acc.push(ComboDefinition {
                    name: node.name.clone(),
                    key_positions,
                    timeout_ms,
                    bindings,
                    layers,
                    description,
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

fn parse_numeric_value(raw: &str) -> Option<u32> {
    raw.trim_matches(['<', '>', ';', ' ']).parse().ok()
}

fn parse_layers(node: &DtNode) -> Vec<u32> {
    node.properties
        .iter()
        .find(|prop| prop.name == "layers")
        .map(|prop| parse_numeric_list(&prop.value.raw))
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

fn parse_numeric_list(raw: &str) -> Vec<u32> {
    raw.replace('<', " ")
        .replace('>', " ")
        .replace(';', " ")
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

    pub fn behaviors(&self) -> Vec<BehaviorDefinition> {
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

    pub fn set_layer_bindings(
        &mut self,
        layer: &str,
        bindings: &[String],
    ) -> Result<(), ProviderError> {
        let refs: Vec<&str> = bindings.iter().map(|value| value.as_str()).collect();
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.set_layer_bindings(layer, &refs)?;
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

    pub fn reorder_layer(&mut self, layer: &str, new_index: usize) -> Result<(), ProviderError> {
        let mut provider = KeymapProvider::new(self.document.clone());
        provider.move_layer_to_index(layer, new_index)?;
        self.document = provider.into_document();
        Ok(())
    }

    pub fn layer_names(&self) -> Vec<String> {
        KeymapProvider::new(self.document.clone()).layer_names()
    }

    pub fn into_document(self) -> DtsDocument {
        self.document
    }
}
