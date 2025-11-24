use std::{fs, path::PathBuf};

use tempfile::tempdir;
use zmk_layout_rs::adapters::{pipeline::AdapterPipeline, standard::TemplateParseMode};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from("tests/fixtures").join(name)
}

#[test]
fn pipeline_loads_from_json_path() {
    let json = r#"{"layers":[{"name":"base","bindings":["&kp A"]}],"combos":[],"behaviors":[],"macros":[],"input_listeners":[],"metadata":{}}"#;
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("layout.json");
    fs::write(&path, json).expect("write");

    let layout = AdapterPipeline::from_json_path(&path).load().expect("load");
    assert_eq!(layout.layers.len(), 1);
    assert_eq!(layout.layers[0].bindings, vec!["&kp A"]);
}

#[test]
fn pipeline_loads_from_dts_with_template_metadata() {
    let template = "{{ rendered_layers }}";
    let rendered = "keymap { base { bindings = < &kp A >; }; };";

    let layout = AdapterPipeline::from_dts_text(rendered)
        .template_source(template)
        .template_mode(TemplateParseMode::StripPlaceholders)
        .load()
        .expect("load");
    assert!(!layout.layers.is_empty());
}

#[test]
fn pipeline_accepts_json_text_directly() {
    let json = r#"{"layers":[{"name":"base","bindings":["&kp ESC"]}],"combos":[],"behaviors":[],"macros":[],"input_listeners":[],"metadata":{}}"#;
    let layout = AdapterPipeline::from_json_text(json).load().expect("load");
    assert_eq!(layout.layers[0].bindings, vec!["&kp ESC"]);
}

#[test]
fn pipeline_handles_dts_without_template() {
    let rendered = fs::read_to_string(fixture("cli_base.dts")).expect("fixture");
    let layout = AdapterPipeline::from_dts_text(rendered)
        .load()
        .expect("load");
    assert!(
        !layout.layers.is_empty(),
        "expected at least one layer from DTS parse"
    );
}
