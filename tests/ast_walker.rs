use zmk_layout_rs::ast::{AstWalker, DtItem, DtNode, DtProperty, DtValue};
use zmk_layout_rs::tokenizer::TokenSpan;

fn span() -> TokenSpan {
    TokenSpan::new(0, 0, 1, 1, 1, 1)
}

fn text_value(raw: &str) -> DtValue {
    DtValue {
        raw: raw.to_string(),
        span: span(),
    }
}

fn property(name: &str, raw: &str) -> DtProperty {
    DtProperty {
        name: name.to_string(),
        raw_name: name.to_string(),
        value: text_value(raw),
        span: span(),
        leading_comments: Vec::new(),
        trailing_comment: None,
    }
}

fn node(name: &str) -> DtNode {
    DtNode {
        name: name.to_string(),
        raw_name: name.to_string(),
        span: span(),
        properties: Vec::new(),
        children: Vec::new(),
        leading_comments: Vec::new(),
        trailing_comments: Vec::new(),
    }
}

#[test]
fn walker_collects_all_nodes() {
    let mut keymap = node("keymap");
    let mut layer = node("default_layer");
    layer.properties.push(property("bindings", "< &kp A >"));
    keymap.children.push(DtItem::Node(layer));
    let items = vec![DtItem::Node(keymap)];
    let walker = AstWalker::new(&items);
    let nodes = walker.nodes();
    assert_eq!(nodes.len(), 2);
}

#[test]
fn walker_filters_by_name() {
    let mut root = node("keymap");
    let mut combos = node("combos");
    combos
        .properties
        .push(property("compatible", "\"zmk,combos\""));
    root.children.push(DtItem::Node(combos));
    let items = vec![DtItem::Node(root)];
    let walker = AstWalker::new(&items);
    let matches = walker.find_nodes_by_name("combos");
    assert_eq!(matches.len(), 1);
    assert!(matches[0].path.ends_with("/combos"));
}

#[test]
fn walker_finds_properties() {
    let mut node = node("macros");
    node.properties
        .push(property("compatible", "\"zmk,behavior-macro\""));
    let items = vec![DtItem::Node(node)];
    let walker = AstWalker::new(&items);
    let props = walker.find_properties_by_name("compatible");
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].1.name, "compatible");
}
