use std::path::PathBuf;

use zmk_layout_rs::{
    dts::DtsDocument,
    macro_support::{MacroError, collect_macros},
};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.dts", name));
    std::fs::read_to_string(path).expect("fixture present")
}

#[test]
fn collect_macros_from_document() -> Result<(), Box<dyn std::error::Error>> {
    let doc = DtsDocument::parse_str(&fixture("serialization_macro"))?;
    let registry = doc.collect_macros()?;
    let def = registry.get("MAKE_LAYER").expect("macro defined");
    assert_eq!(def.args, vec!["name"]);
    assert!(def.body.contains("bindings"));
    Ok(())
}

#[test]
fn duplicate_macros_error() {
    let source = "#define FOO 1\n#define FOO 2\n";
    let doc = DtsDocument::parse_str(source).expect("parsed");
    let err = doc.collect_macros().expect_err("duplicates fail");
    match err {
        MacroError::DuplicateDefinition { name, .. } => assert_eq!(name, "FOO"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn macros_inside_nodes_are_collected() -> Result<(), Box<dyn std::error::Error>> {
    let doc = DtsDocument::parse_str(&fixture("dts_roundtrip_keymap"))?;
    let registry = collect_macros(&doc.items)?;
    assert!(registry.get("LSYM").is_some());
    Ok(())
}
