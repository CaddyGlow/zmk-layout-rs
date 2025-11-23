use crate::{
    ast::{DtComment, DtItem, DtNode, DtProperty, DtValue},
    tokenizer::TokenSpan,
};

use super::{ProviderError, COMBO_CONDITION_COMMENT_PREFIX};

pub fn find_layer_node<'a>(items: &'a [DtItem], name: &str) -> Option<&'a DtNode> {
    for item in items {
        match item {
            DtItem::Node(node) => {
                if node.name == name {
                    return Some(node);
                }
                if let Some(child) = find_layer_node(&node.children, name) {
                    return Some(child);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn find_layer_node_mut<'a>(items: &'a mut [DtItem], name: &str) -> Option<&'a mut DtNode> {
    for item in items {
        match item {
            DtItem::Node(node) => {
                if node.name == name {
                    return Some(node);
                }
                if let Some(child) = find_layer_node_mut(&mut node.children, name) {
                    return Some(child);
                }
            }
            _ => {}
        }
    }
    None
}

#[allow(dead_code)]
pub fn find_child_node<'a>(items: &'a [DtItem], name: &str) -> Option<&'a DtNode> {
    items
        .iter()
        .find_map(|item| match item {
            DtItem::Node(node) if node.name == name => Some(node),
            _ => None,
        })
}

#[allow(dead_code)]
pub fn find_child_node_mut<'a>(node: &'a mut DtNode, name: &str) -> Option<&'a mut DtNode> {
    node.children
        .iter_mut()
        .find_map(|child| match child {
            DtItem::Node(child) if child.name == name => Some(child),
            _ => None,
        })
}

pub fn find_bindings_property(node: &DtNode) -> Option<&DtProperty> {
    node.properties
        .iter()
        .find(|prop| prop.name == "bindings")
}

pub fn find_bindings_property_mut(node: &mut DtNode) -> Option<&mut DtProperty> {
    node.properties
        .iter_mut()
        .find(|prop| prop.name == "bindings")
}

pub fn ensure_bindings_property(node: &mut DtNode) -> &mut DtProperty {
    if let Some(idx) = node
        .properties
        .iter()
        .position(|prop| prop.name == "bindings")
    {
        return node
            .properties
            .get_mut(idx)
            .expect("bindings property should exist");
    }
    node.properties.push(DtProperty {
        name: "bindings".to_string(),
        raw_name: String::new(),
        value: DtValue {
            raw: "< >".to_string(),
            span: empty_span(),
        },
        span: empty_span(),
        leading_comments: Vec::new(),
        trailing_comment: None,
    });
    let idx = node.properties.len() - 1;
    node.properties
        .get_mut(idx)
        .expect("bindings property just inserted")
}

pub fn ensure_property<'a>(node: &'a mut DtNode, name: &str) -> &'a mut DtProperty {
    if let Some(idx) = node
        .properties
        .iter()
        .position(|prop| prop.name == name)
    {
        return node
            .properties
            .get_mut(idx)
            .expect("property should exist");
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
    let idx = node.properties.len() - 1;
    node.properties
        .get_mut(idx)
        .expect("property just inserted")
}

pub fn apply_combo_conditions(node: &mut DtNode, conditions: &[String]) {
    node.leading_comments
        .retain(|comment| !is_condition_comment(comment));
    if conditions.is_empty() {
        return;
    }
    node.leading_comments
        .extend(conditions.iter().map(|condition| DtComment {
            text: format!("{} {}", COMBO_CONDITION_COMMENT_PREFIX, condition),
            span: empty_span(),
        }));
}

pub fn is_behavior_root(node: &DtNode) -> bool {
    node.name == "behaviors" || node.name == "macros"
}

pub fn empty_node(name: &str) -> DtNode {
    DtNode {
        name: name.to_string(),
        raw_name: String::new(),
        span: empty_span(),
        properties: Vec::new(),
        children: Vec::new(),
        leading_comments: Vec::new(),
        trailing_comments: Vec::new(),
    }
}

pub fn empty_span() -> TokenSpan {
    super::format::empty_span()
}

pub fn ensure_layer_node(
    document_items: &mut [DtItem],
    layer: &str,
) -> Result<(), ProviderError> {
    let keymap_root = find_layer_node_mut(document_items, "keymap")
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
    let new_node = DtNode {
        name: layer.to_string(),
        raw_name: String::new(),
        span: empty_span(),
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

fn is_condition_comment(comment: &DtComment) -> bool {
    comment
        .text
        .trim_start()
        .starts_with(COMBO_CONDITION_COMMENT_PREFIX)
}
