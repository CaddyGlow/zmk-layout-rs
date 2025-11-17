use std::{env, error::Error, fs, path::PathBuf};

use zmk_layout_rs::{ast::DtItem, dts::DtsDocument, serialization::SerializeConfig};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.dts", name))
}

#[test]
fn document_round_trip_in_memory() -> Result<(), Box<dyn Error>> {
    let source = fs::read_to_string(fixture_path("dts_roundtrip_keymap"))?;
    let doc = DtsDocument::parse_str(&source)?;
    let output = doc.to_string_with_config(SerializeConfig { indent: "    " })?;
    assert_eq!(output, source);
    Ok(())
}

#[test]
fn document_parse_and_write_files() -> Result<(), Box<dyn Error>> {
    let fixture = fixture_path("serialization_formatted");
    let original = fs::read_to_string(&fixture)?;
    let doc = DtsDocument::parse_file(&fixture)?;

    let tmp_path = env::temp_dir().join(format!(
        "zmk-layout-dts-{}.dts",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    doc.write_to_file(&tmp_path)?;
    let written = fs::read_to_string(&tmp_path)?;
    fs::remove_file(&tmp_path)?;
    assert_eq!(written, original);
    Ok(())
}

#[test]
fn document_macro_expansion_helper() -> Result<(), Box<dyn Error>> {
    let source = fs::read_to_string(fixture_path("serialization_macro"))?;
    let mut doc = DtsDocument::parse_str(&source)?;
    let call = find_macro_call(&doc.items).cloned().expect("call present");
    let expanded = doc.expand_macro_call(&call)?;
    assert!(expanded.contains("bindings"));
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
