//! High-level helpers for mutating keymap documents.

use thiserror::Error;

use crate::{
    ast::{DtItem, DtNode, DtProperty, DtValue},
    bindings::{BindingParser, LayoutBinding},
    dts::DtsDocument,
    tokenizer::TokenSpan,
};

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

    fn combo_node_mut(&mut self, combo: &str) -> Result<&mut DtNode, ProviderError> {
        let combos_root = find_layer_node_mut(&mut self.document.items, "combos")
            .ok_or(ProviderError::CombosMissing)?;
        find_child_node_mut(combos_root, combo)
            .ok_or_else(|| ProviderError::ComboNotFound(combo.to_string()))
    }

    fn behavior_node_mut(&mut self, behavior: &str) -> Result<&mut DtNode, ProviderError> {
        let behaviors_root = find_layer_node_mut(&mut self.document.items, "behaviors")
            .ok_or(ProviderError::BehaviorsMissing)?;
        find_child_node_mut(behaviors_root, behavior)
            .ok_or_else(|| ProviderError::BehaviorNotFound(behavior.to_string()))
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

fn empty_span() -> TokenSpan {
    TokenSpan::new(0, 0, 1, 1, 1, 1)
}

fn parse_binding_list(raw: &str) -> Vec<String> {
    let mut inner = raw.trim();
    if inner.starts_with('<') {
        inner = inner.trim_start_matches('<').trim_start();
    }
    if inner.ends_with('>') {
        inner = inner.trim_end_matches('>').trim_end();
    }
    if inner.ends_with(';') {
        inner = inner.trim_end_matches(';').trim_end();
    }

    let mut bindings = Vec::new();
    let mut current = String::new();
    for token in inner.split_whitespace() {
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
                });
            }
            for child in &node.children {
                collect_behaviors(child, acc);
            }
        }
        _ => {}
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
        .map(|prop| prop.value.raw.clone())
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
        .map(|prop| parse_binding_list(&prop.value.raw))
        .unwrap_or_default()
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
                acc.push(ComboDefinition {
                    name: node.name.clone(),
                    key_positions,
                    timeout_ms,
                    bindings,
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
}
