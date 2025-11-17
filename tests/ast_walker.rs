use std::path::PathBuf;

use zmk_layout_rs::ast::{AstWalker, DtItem, DtNode, DtProperty, DtValue};
use zmk_layout_rs::parser::parse_layout;
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

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.dts", name))
}

#[test]
fn walker_can_be_created() {
    let walker = AstWalker::new(&[]);
    assert_eq!(walker.nodes().len(), 0);
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
fn walker_walks_empty_node() {
    let root = node("root");
    let items = vec![DtItem::Node(root)];
    let walker = AstWalker::new(&items);
    assert_eq!(walker.nodes().len(), 1);
}

#[test]
fn walker_simple_keymap() {
    let mut root = node("/");
    let mut keymap = node("keymap");
    let mut layer = node("default_layer");
    layer
        .properties
        .push(property("bindings", "< &kp A &kp B >"));
    keymap.children.push(DtItem::Node(layer));
    root.children.push(DtItem::Node(keymap));
    let items = vec![DtItem::Node(root)];
    let walker = AstWalker::new(&items);
    assert_eq!(walker.nodes().len(), 3);
}

#[test]
fn walker_with_behaviors() {
    let mut root = node("/");
    let mut behaviors = node("behaviors");
    let mut ht = node("ht");
    ht.properties
        .push(property("compatible", "\"zmk,behavior-hold-tap\""));
    behaviors.children.push(DtItem::Node(ht));
    root.children.push(DtItem::Node(behaviors));
    let items = vec![DtItem::Node(root)];
    let walker = AstWalker::new(&items);
    assert_eq!(walker.find_nodes_by_name("ht").len(), 1);
}

#[test]
fn walker_with_combos() {
    let mut root = node("/");
    let mut combos = node("combos");
    let mut combo = node("combo_esc");
    combo.properties.push(property("bindings", "< &kp ESC >"));
    combos.children.push(DtItem::Node(combo));
    root.children.push(DtItem::Node(combos));
    let items = vec![DtItem::Node(root)];
    let walker = AstWalker::new(&items);
    assert_eq!(walker.find_nodes_by_name("combo_esc").len(), 1);
}

#[test]
fn walker_with_macros() {
    let mut root = node("/");
    let mut macros = node("macros");
    let mut macro_node = node("hello");
    macro_node
        .properties
        .push(property("bindings", "< &kp H &kp E >"));
    macros.children.push(DtItem::Node(macro_node));
    root.children.push(DtItem::Node(macros));
    let items = vec![DtItem::Node(root)];
    let walker = AstWalker::new(&items);
    assert_eq!(walker.find_nodes_by_name("hello").len(), 1);
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

#[test]
fn walker_with_nested_properties() {
    let mut root = node("/");
    let mut config = node("config");
    config.properties.push(property("deep-sleep-enable", ""));
    root.children.push(DtItem::Node(config));
    let items = vec![DtItem::Node(root)];
    let walker = AstWalker::new(&items);
    assert!(
        !walker
            .find_properties_by_name("deep-sleep-enable")
            .is_empty()
    );
}

#[test]
fn walker_complex_fixture() {
    let source =
        std::fs::read_to_string(fixture_path("ast_walker_complex")).expect("fixture present");
    let items = parse_layout(&source).expect("parse fixture");
    let walker = AstWalker::new(&items);
    assert!(!walker.find_nodes_by_name("keymap").is_empty());
    assert!(!walker.find_nodes_by_name("behaviors").is_empty());
}
