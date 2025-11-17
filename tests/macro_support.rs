use std::path::PathBuf;

use zmk_layout_rs::{
    ast::DtItem,
    dts::DtsDocument,
    macro_support::{MacroError, MacroExpansionError, collect_macros},
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

#[test]
fn expand_macro_call_with_args() -> Result<(), Box<dyn std::error::Error>> {
    let doc = DtsDocument::parse_str(&fixture("serialization_macro"))?;
    let registry = doc.collect_macros()?;
    let call = find_macro_call(&doc.items).expect("call present");
    let expanded = registry.expand_call(call)?;
    assert!(expanded.starts_with("nav {"));
    assert!(expanded.contains("&none"));
    Ok(())
}

#[test]
fn macro_call_unknown_name() -> Result<(), Box<dyn std::error::Error>> {
    let doc = DtsDocument::parse_str("#define FOO() 1\nBAR()\n")?;
    let registry = doc.collect_macros()?;
    let call = find_macro_call(&doc.items).expect("call present");
    let err = registry.expand_call(call).expect_err("unknown macro fails");
    assert!(matches!(err, MacroExpansionError::UnknownMacro { .. }));
    Ok(())
}

#[test]
fn macro_call_arg_mismatch() -> Result<(), Box<dyn std::error::Error>> {
    let doc = DtsDocument::parse_str("#define FOO(x) x\nFOO(1, 2)\n")?;
    let registry = doc.collect_macros()?;
    let call = find_macro_call(&doc.items).expect("call present");
    let err = registry.expand_call(call).expect_err("mismatch error");
    assert!(matches!(err, MacroExpansionError::ArgCountMismatch { .. }));
    Ok(())
}

fn find_macro_call<'a>(items: &'a [DtItem]) -> Option<&'a zmk_layout_rs::ast::DtMacroCall> {
    for item in items {
        match item {
            DtItem::MacroCall(call) => return Some(call),
            DtItem::Node(node) => {
                if let Some(call) = find_macro_call(&node.children) {
                    return Some(call);
                }
            }
            DtItem::Conditional(cond) => {
                for branch in &cond.branches {
                    if let Some(call) = find_macro_call(&branch.items) {
                        return Some(call);
                    }
                }
            }
            _ => {}
        }
    }
    None
}
