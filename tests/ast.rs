use std::path::PathBuf;

use zmk_layout_rs::{
    ast::{DtItem, DtNode, TemplateKind},
    parser::parse_layout,
    tokenizer::LayoutError,
};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.dts", name));
    std::fs::read_to_string(path).expect("fixture present")
}

fn expect_node<'a>(item: &'a DtItem, name: &str) -> &'a DtNode {
    match item {
        DtItem::Node(node) => {
            assert_eq!(node.name, name, "unexpected node name");
            node
        }
        other => panic!("expected node `{name}`, got {other:?}"),
    }
}

#[test]
fn parse_simple_structure() -> Result<(), LayoutError> {
    let source = fixture("ast_simple");
    let items = parse_layout(&source)?;
    assert_eq!(items.len(), 1);

    let keymap = expect_node(&items[0], "keymap");
    assert_eq!(keymap.properties.len(), 1);
    assert_eq!(keymap.properties[0].name, "compatible");
    assert!(keymap.properties[0].value.raw.contains("\"zmk,keymap\""));

    let default = keymap
        .children
        .iter()
        .find_map(|child| match child {
            DtItem::Node(node) if node.name == "default_layer" => Some(node),
            _ => None,
        })
        .expect("default_layer node present");
    assert_eq!(default.properties.len(), 1);
    assert!(default.properties[0].value.raw.contains("&kp Q"));
    Ok(())
}

#[test]
fn parse_comments_attach() -> Result<(), LayoutError> {
    let source = fixture("ast_comments");
    let items = parse_layout(&source)?;
    let base = expect_node(&items[0], "base");
    assert_eq!(base.leading_comments.len(), 1);
    assert!(base.leading_comments[0].text.contains("base layer"));

    let prop = &base.properties[0];
    assert_eq!(prop.name, "bindings");
    assert!(
        prop.trailing_comment
            .as_ref()
            .expect("inline comment")
            .text
            .contains("homerow tap")
    );
    Ok(())
}

#[test]
fn parse_conditionals() -> Result<(), LayoutError> {
    let source = fixture("ast_conditionals");
    let items = parse_layout(&source)?;
    assert_eq!(items.len(), 1);
    let conditional = match &items[0] {
        DtItem::Conditional(cond) => cond,
        other => panic!("expected conditional item, got {other:?}"),
    };
    assert_eq!(conditional.branches.len(), 2);
    assert!(conditional.branches[0].directive.text.starts_with("#if"));
    assert!(conditional.end_directive.text.starts_with("#endif"));
    let branch_node = expect_node(&conditional.branches[0].items[0], "layer_macro");
    assert!(branch_node.properties[0].value.raw.contains("&none"));
    let else_node = expect_node(&conditional.branches[1].items[0], "layer_macro");
    assert!(else_node.properties[0].value.raw.contains("&kp ESC"));
    Ok(())
}

#[test]
fn parse_template_fragments() -> Result<(), LayoutError> {
    let source = fixture("ast_template");
    let items = parse_layout(&source)?;
    assert!(matches!(
        &items[0],
        DtItem::Template(t) if matches!(t.kind, TemplateKind::Block)
    ));
    assert!(matches!(
        &items[1],
        DtItem::Template(t) if matches!(t.kind, TemplateKind::Block)
    ));
    let node = expect_node(&items[2], "layer_");
    assert!(
        node.properties[0]
            .value
            .raw
            .contains("{{ bindings[i] | join(\" \") }}")
    );
    assert!(matches!(
        items.last().unwrap(),
        DtItem::Template(t) if matches!(t.kind, TemplateKind::Block)
    ));
    Ok(())
}

#[test]
fn parse_macros() -> Result<(), LayoutError> {
    let source = fixture("ast_macro_usage");
    let items = parse_layout(&source)?;
    assert!(matches!(items[0], DtItem::Macro(_)));
    let macros_node = expect_node(&items[1], "macros");
    assert!(macros_node.properties[0].value.raw.contains("LSYM"));
    Ok(())
}

#[test]
fn parse_root_node() -> Result<(), LayoutError> {
    let items = parse_layout(&fixture("ast_root"))?;
    let root = expect_node(&items[0], "/");
    assert_eq!(root.children.len(), 1);
    let macros = expect_node(&root.children[0], "macros");
    assert!(macros.children.is_empty());
    Ok(())
}

#[test]
fn parse_labeled_nodes() -> Result<(), LayoutError> {
    let items = parse_layout(&fixture("ast_labeled_nodes"))?;
    let root = expect_node(&items[0], "/");
    let behaviors = expect_node(&root.children[0], "behaviors");
    let lower = expect_node(&behaviors.children[0], "lower");
    assert_eq!(lower.properties[0].name, "compatible");
    assert!(
        lower
            .properties
            .iter()
            .any(|prop| prop.name == "hold-trigger-on-release" && prop.value.raw.is_empty())
    );

    let root_two = expect_node(&items[1], "/");
    let macros = expect_node(&root_two.children[0], "macros");
    let macro_node = expect_node(&macros.children[0], "rgb_ug_status_macro");
    let prop_names: Vec<_> = macro_node
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(prop_names.contains(&"label"));
    assert!(prop_names.contains(&"compatible"));
    assert!(prop_names.contains(&"#binding-cells"));

    let root_three = expect_node(&items[2], "/");
    let patch = expect_node(&root_three.children[1], "patch");
    let relabel = patch
        .children
        .iter()
        .find_map(|item| match item {
            DtItem::Node(node) if node.raw_name.contains("relabel") => Some(node),
            _ => None,
        })
        .expect("relabel node present");
    assert!(relabel.raw_name.contains("&target"));
    Ok(())
}

#[test]
fn parse_malformed_reports_error() {
    let source = fixture("ast_malformed");
    let err = parse_layout(&source).expect_err("should fail");
    assert!(matches!(err, LayoutError::Parse { .. }));
}

#[test]
fn parse_preprocessor_conditionals() -> Result<(), LayoutError> {
    let items = parse_layout(&fixture("ast_preprocessor"))?;
    let root = expect_node(&items[0], "/");
    let behaviors = expect_node(&root.children[0], "behaviors");
    let left = expect_node(&behaviors.children[0], "left");
    let conditional = match &left.children[0] {
        DtItem::Conditional(cond) => cond,
        other => panic!("expected conditional, got {other:?}"),
    };
    let inner = match &conditional.branches[0].items[0] {
        DtItem::Conditional(cond) => cond,
        other => panic!("expected inner conditional, got {other:?}"),
    };
    assert!(inner.branches[0].items.iter().any(|item| {
        matches!(item, DtItem::Property(prop) if prop.name == "hold-while-undecided")
    }));
    Ok(())
}

#[test]
fn parse_reference_nodes() -> Result<(), LayoutError> {
    let items = parse_layout(&fixture("ast_reference_nodes"))?;
    let root = expect_node(&items[0], "/");
    let config = expect_node(&root.children[0], "config");
    let reference = match &config.children[0] {
        DtItem::Node(node) => node,
        other => panic!("expected node, got {other:?}"),
    };
    assert_eq!(reference.name, "&existing");
    Ok(())
}
