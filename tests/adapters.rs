use std::{error::Error, fs, path::PathBuf};

use serde_json::json;
use zmk_layout_rs::adapters::{
    AdapterLayout, export_standard_file, export_standard_str, import_standard_file,
    import_standard_str, import_standard_str_with_template,
};
use zmk_layout_rs::dts::DtsDocument;
use zmk_layout_rs::providers::KeymapProvider;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.dts", name));
    fs::read_to_string(path).expect("fixture exists")
}

#[test]
fn adapter_extracts_combo_and_behavior_specs() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("ast_walker_complex"))?;
    let layout = AdapterLayout::from_document(&doc);

    assert_eq!(layout.layers.len(), 1);
    let layer = &layout.layers[0];
    assert_eq!(layer.name, "default_layer");
    assert_eq!(layer.bindings, vec!["&kp A", "&kp B"]);

    assert_eq!(layout.combos.len(), 1);
    let combo = &layout.combos[0];
    assert_eq!(combo.name, "combo_esc");
    assert_eq!(combo.key_positions, vec![0, 1]);
    assert_eq!(combo.bindings, vec!["&kp ESC"]);

    assert_eq!(layout.behaviors.len(), 2);
    let ht = layout
        .behaviors
        .iter()
        .find(|behavior| behavior.name == "ht")
        .expect("hold-tap behavior present");
    assert_eq!(ht.bindings, vec!["&kp ESC", "&kp TAB"]);
    Ok(())
}

#[test]
fn adapter_applies_mutations_via_provider() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("ast_walker_complex"))?;
    let mut provider = KeymapProvider::new(doc);
    let mut layout = AdapterLayout::from_provider(&provider);

    let combo = layout
        .combos
        .iter_mut()
        .find(|combo| combo.name == "combo_esc")
        .expect("combo exists");
    combo.bindings = vec!["&kp SPACE".into()];
    combo.key_positions = vec![2, 3];
    combo.timeout_ms = Some(90);

    let ht = layout
        .behaviors
        .iter_mut()
        .find(|behavior| behavior.name == "ht")
        .expect("behavior exists");
    ht.bindings = vec!["&kp Z".into(), "&kp X".into()];

    let layer = layout
        .layers
        .iter_mut()
        .find(|layer| layer.name == "default_layer")
        .expect("layer exists");
    layer.bindings = vec!["&kp 1".into(), "&kp 2".into()];

    layout.apply_to_provider(&mut provider)?;
    let updated = provider.into_document().to_string()?;

    assert!(updated.contains("&kp 1 &kp 2"));
    assert!(updated.contains("timeout-ms = < 90 >;"));
    assert!(updated.contains("bindings = < &kp SPACE >;"));
    assert!(updated.contains("key-positions = < 2 3 >;"));
    assert!(updated.contains("bindings = < &kp Z &kp X >;"));
    Ok(())
}

#[test]
fn adapter_round_trips_standard_json() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("ast_walker_complex"))?;
    let mut layout = AdapterLayout::from_document(&doc);
    layout.metadata.title = Some("Adapter Fixture".into());
    layout.metadata.author = Some("Tester".into());
    layout
        .metadata
        .extras
        .insert("custom".into(), json!({"value": 42}));

    let json = layout.to_standard_json()?;
    assert!(json.contains("Adapter Fixture"));
    let parsed = AdapterLayout::from_standard_json(&json)?;
    assert_eq!(parsed, layout);
    Ok(())
}

#[test]
fn adapter_extracts_input_listeners_from_dts() -> Result<(), Box<dyn Error>> {
    let source = r#"
&mmv_input_listener {
    input-processors = <&zip_xy_scaler 4 5>;
    // Cursor Layer
    Cursor {
        layers = <2 3>;
        input-processors = <&zip_xy_scaler 1 9>, <&zip_scroll_scaler 2>;
    };
};
"#;
    let doc = DtsDocument::parse_str(source)?;
    let layout = AdapterLayout::from_document(&doc);
    assert_eq!(layout.input_listeners.len(), 1);
    let listener = &layout.input_listeners[0];
    assert_eq!(listener.code, "&mmv_input_listener");
    assert_eq!(
        listener.input_processors[0].params,
        vec![json!(4), json!(5)]
    );
    assert_eq!(listener.nodes.len(), 1);
    let node = &listener.nodes[0];
    assert_eq!(node.code, "Cursor");
    assert_eq!(node.description.as_deref(), Some("Cursor Layer"));
    assert_eq!(node.layers, vec![2, 3]);
    assert_eq!(node.input_processors[0].params, vec![json!(1), json!(9)]);
    assert_eq!(node.input_processors[1].code, "&zip_scroll_scaler");
    assert_eq!(node.input_processors[1].params, vec![json!(2)]);
    Ok(())
}

#[test]
fn adapter_imports_input_listeners_from_json() -> Result<(), Box<dyn Error>> {
    let json = json!({
        "title": "Listener Test",
        "inputListeners": [{
            "code": "&mmv_input_listener",
            "inputProcessors": [],
            "nodes": [{
                "code": "LAYER_MouseSlow",
                "description": "Mouse Slow",
                "layers": [1],
                "inputProcessors": [{
                    "code": "&zip_xy_scaler",
                    "params": [1, 9]
                }]
            }]
        }]
    })
    .to_string();

    let layout = AdapterLayout::from_standard_json(&json)?;
    assert_eq!(layout.input_listeners.len(), 1);
    assert_eq!(layout.input_listeners[0].nodes.len(), 1);

    let template = "/* Input Listeners */\n{{input_listeners}}\n";
    let rendered = import_standard_str_with_template(&json, template)?.to_string()?;
    assert!(
        rendered.contains("&mmv_input_listener"),
        "listener block rendered"
    );
    assert!(
        rendered.contains("input-processors = <&zip_xy_scaler 1 9>;"),
        "processor params rendered"
    );
    Ok(())
}

#[test]
fn adapter_file_io_helpers_round_trip() -> Result<(), Box<dyn Error>> {
    let base = DtsDocument::parse_str(&fixture("ast_walker_complex"))?;
    let mut provider = KeymapProvider::new(base.clone());
    provider.set_binding("default_layer", 0, "&kp V")?;
    let mutated = provider.into_document();

    let path = std::env::temp_dir().join("adapter_round_trip.json");
    export_standard_file(&mutated, &path)?;
    let imported = import_standard_file(&path, base.clone())?;
    assert_eq!(imported.to_string()?, mutated.to_string()?);

    let json = export_standard_str(&mutated)?;
    let imported = import_standard_str(&json, base)?;
    assert_eq!(imported.to_string()?, mutated.to_string()?);
    Ok(())
}

#[test]
fn adapter_supports_simple_templates() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("ast_walker_complex"))?;
    let mut layout = AdapterLayout::from_document(&doc);
    layout.metadata.title = Some("MoErgo Layout".into());
    layout
        .metadata
        .extras
        .insert("includes".into(), json!("#include <behaviors.dtsi>"));
    let json = layout.to_standard_json()?;

    let template = r#"
{{includes}}
{{layer_names_defines}}
{{combos}}
/ {
    keymap {
{{rendered_layers}}
    };
};
"#;

    let imported = import_standard_str_with_template(&json, template)?;
    let rendered = imported.to_string()?;
    assert!(rendered.contains("#define LAYER_DEFAULT_LAYER 0"));
    assert!(rendered.contains("combo_combo_esc"));
    assert!(rendered.contains("#include <behaviors.dtsi>"));
    assert!(rendered.contains("&kp A &kp B"));
    Ok(())
}

#[test]
fn template_preserves_spacing() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("ast_walker_complex"))?;
    let layout = AdapterLayout::from_document(&doc);
    let json = layout.to_standard_json()?;

    let template = r#"/* Header Comment */

/* Second Comment */

{{layer_names_defines}}

/* Footer Comment */
"#;

    let imported = import_standard_str_with_template(&json, template)?;
    let rendered = imported.to_string()?;

    assert!(rendered.contains("/* Header Comment */\n\n/* Second Comment */"));
    assert!(rendered.contains("/* Second Comment */\n\n#define LAYER_DEFAULT_LAYER 0"));
    assert!(rendered.contains("#define LAYER_DEFAULT_LAYER 0\n\n/* Footer Comment */"));
    Ok(())
}
