use crate::{ast::DtItem, dts::DtsDocument};

use super::{
    combos::{
        behavior_description, binding_cells_value, binding_list_from_node, capture_node_properties,
        compatible_value, parse_optional_numeric_property,
    },
};

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
    pub properties: Vec<super::combos::NodeProperty>,
}

fn collect_behaviors(item: &DtItem, acc: &mut Vec<BehaviorDefinition>) {
    match item {
        DtItem::Node(node) => {
            if is_behavior_node(node) {
                acc.push(BehaviorDefinition {
                    name: node.name.clone(),
                    compatible: compatible_value(node),
                    binding_cells: binding_cells_value(node),
                    bindings: binding_list_from_node(node),
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

fn is_behavior_node(node: &crate::ast::DtNode) -> bool {
    node.properties.iter().any(|prop| {
        prop.name == "compatible" && prop.value.raw.to_lowercase().contains("behavior-")
    })
}

fn label_value(node: &crate::ast::DtNode) -> Option<String> {
    node.properties
        .iter()
        .find(|prop| prop.name == "label")
        .map(|prop| trim_string_literal(&prop.value.raw))
}

fn trim_string_literal(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed.trim_matches('"').trim().to_string()
}
