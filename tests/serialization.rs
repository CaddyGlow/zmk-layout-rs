use std::{error::Error, path::PathBuf};

use zmk_layout_rs::{
    ast::{DtItem, DtNode, DtProperty, DtValue},
    parser::parse_layout,
    serialization::{SerializeConfig, serialize, serialize_with_config},
    tokenizer::TokenSpan,
};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.dts", name));
    std::fs::read_to_string(path).expect("fixture present")
}

#[test]
fn round_trip_formatted_layout() -> Result<(), Box<dyn Error>> {
    let source = fixture("serialization_formatted");
    let ast = parse_layout(&source)?;
    let output = serialize(&ast)?;
    assert_eq!(output, source);
    Ok(())
}

#[test]
fn retain_directives_and_templates() -> Result<(), Box<dyn Error>> {
    let source = fixture("serialization_directives");
    let ast = parse_layout(&source)?;
    let output = serialize(&ast)?;
    assert_eq!(output, source);
    Ok(())
}

#[test]
fn retain_generic_preprocessor_directives() -> Result<(), Box<dyn Error>> {
    let source = "#pragma once\nkeymap {\n    compatible = \"zmk,keymap\";\n};\n";
    let ast = parse_layout(source)?;
    let output = serialize(&ast)?;
    assert_eq!(
        output,
        "#pragma once\nkeymap {\n  compatible = \"zmk,keymap\";\n};\n"
    );
    Ok(())
}

#[test]
fn macro_definition_and_invocation_preserved() -> Result<(), Box<dyn Error>> {
    let source = fixture("serialization_macro");
    let ast = parse_layout(&source)?;
    let output = serialize(&ast)?;
    assert_eq!(output, source);
    Ok(())
}

#[test]
fn targeted_mutation_only_changes_value() -> Result<(), Box<dyn Error>> {
    let source = fixture("serialization_formatted");
    let mut ast = parse_layout(&source)?;
    let keymap = expect_node_mut(&mut ast, "keymap");
    let prop = keymap
        .properties
        .iter_mut()
        .find(|prop| prop.name == "compatible")
        .expect("compatible property present");
    prop.value.raw = "\"zmk,keymap-updated\"".to_string();
    let output = serialize(&ast)?;
    let expected = source.replace("\"zmk,keymap\"", "\"zmk,keymap-updated\"");
    assert_eq!(output, expected);
    Ok(())
}

#[test]
fn serialization_writes_flag_properties() {
    let span = TokenSpan::new(0, 0, 1, 1, 1, 1);
    let node = DtNode {
        name: "broken".into(),
        raw_name: String::new(),
        span,
        properties: vec![DtProperty {
            name: "example".into(),
            raw_name: String::new(),
            value: DtValue {
                raw: String::new(),
                span,
            },
            span,
            leading_comments: vec![],
            trailing_comment: None,
        }],
        children: Vec::new(),
        leading_comments: vec![],
        trailing_comments: vec![],
    };

    let output = serialize(&[DtItem::Node(node.clone())]).expect("serialize flag property");
    assert_eq!(output, "broken {\n  example;\n};\n");

    let output = serialize_with_config(&[DtItem::Node(node)], SerializeConfig { indent: "\t" })
        .expect("serialize flag property with custom indent");
    assert_eq!(output, "broken {\n\texample;\n};\n");
}

fn expect_node_mut<'a>(items: &'a mut [DtItem], name: &str) -> &'a mut DtNode {
    for item in items.iter_mut() {
        if let DtItem::Node(node) = item {
            if node.name == name {
                return node;
            }
        }
    }
    panic!("node {name} not found");
}
