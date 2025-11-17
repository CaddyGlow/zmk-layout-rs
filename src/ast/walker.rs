//! Helpers to traverse `DtNode` trees.

use super::{DtItem, DtNode, DtProperty};

/// Reference to a node including its computed path.
#[derive(Debug, Clone)]
pub struct NodeRef<'a> {
    pub node: &'a DtNode,
    pub path: String,
}

/// AST walker capable of traversing the document tree.
pub struct AstWalker<'a> {
    items: &'a [DtItem],
}

impl<'a> AstWalker<'a> {
    pub fn new(items: &'a [DtItem]) -> Self {
        Self { items }
    }

    /// Return all nodes reachable from the walker.
    pub fn nodes(&self) -> Vec<NodeRef<'a>> {
        let mut nodes = Vec::new();
        for item in self.items {
            if let DtItem::Node(node) = item {
                collect_nodes(node, String::new(), &mut nodes);
            }
        }
        nodes
    }

    pub fn find_nodes<F>(&self, predicate: F) -> Vec<NodeRef<'a>>
    where
        F: Fn(&NodeRef<'a>) -> bool,
    {
        self.nodes()
            .into_iter()
            .filter(|node| predicate(node))
            .collect()
    }

    pub fn find_nodes_by_name(&self, name: &str) -> Vec<NodeRef<'a>> {
        self.find_nodes(|node| node.node.name == name)
    }

    pub fn find_nodes_by_path(&self, pattern: &str) -> Vec<NodeRef<'a>> {
        self.find_nodes(|node| node.path.contains(pattern))
    }

    pub fn find_nodes_by_compatible(&self, needle: &str) -> Vec<NodeRef<'a>> {
        self.find_nodes(|node| {
            node.node.properties.iter().any(|prop| {
                prop.name == "compatible"
                    && prop
                        .value
                        .raw
                        .to_lowercase()
                        .contains(&needle.to_lowercase())
            })
        })
    }

    pub fn find_properties_by_name(&self, name: &str) -> Vec<(&'a DtNode, &'a DtProperty)> {
        let mut results = Vec::new();
        for item in self.items {
            if let DtItem::Node(node) = item {
                collect_props(node, name, &mut results);
            }
        }
        results
    }
}

fn collect_nodes<'a>(node: &'a DtNode, parent_path: String, acc: &mut Vec<NodeRef<'a>>) {
    let current_path = if parent_path.is_empty() {
        format!("/{}", node.name)
    } else if parent_path == "/" {
        format!("/{}", node.name)
    } else {
        format!("{}/{}", parent_path.trim_end_matches('/'), node.name)
    };
    acc.push(NodeRef {
        node,
        path: current_path.clone(),
    });
    for child in &node.children {
        if let DtItem::Node(inner) = child {
            collect_nodes(inner, current_path.clone(), acc);
        }
    }
}

fn collect_props<'a>(node: &'a DtNode, name: &str, acc: &mut Vec<(&'a DtNode, &'a DtProperty)>) {
    for prop in &node.properties {
        if prop.name == name {
            acc.push((node, prop));
        }
    }
    for child in &node.children {
        if let DtItem::Node(inner) = child {
            collect_props(inner, name, acc);
        }
    }
}
