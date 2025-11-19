use std::{error::Error, fs, path::PathBuf};

use serde_json::{Value, json};
use zmk_layout_rs::adapters::{
    AdapterLayout, export_standard_file, export_standard_str, export_standard_str_with_template,
    import_standard_file, import_standard_str, import_standard_str_with_template,
    render_standard_template,
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
    assert_eq!(combo.binding.as_deref(), Some("&kp ESC"));

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
    combo.binding = Some("&kp SPACE".into());
    combo.key_positions = vec![2, 3];
    combo.timeout_ms = Some(90);
    combo.layers = vec![0, 1];

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
    assert!(updated.contains("layers = <0 1>;"));
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
    let rendered = render_standard_template(&json, template)?;
    assert!(
        rendered.contains("&mmv_input_listener"),
        "listener block rendered"
    );
    assert!(
        rendered.contains("\n    // Mouse Slow"),
        "listener comments use four-space indent"
    );
    assert!(
        rendered.contains("input-processors = <&zip_xy_scaler 1 9>;"),
        "processor params rendered"
    );
    Ok(())
}

#[test]
fn combo_descriptions_and_layers_round_trip() -> Result<(), Box<dyn Error>> {
    let source = r#"
/ {
    combos {
        compatible = "zmk,combos";
        // sticky "meh" modifiers (Alt + Ctrl + Shift) - TailorKey
        combo_sticky_meh {
            key-positions = <1 2>;
            bindings = < &kp A >;
            layers = <0 2>;
        };
    };
};
"#;
    let doc = DtsDocument::parse_str(source)?;
    let layout = AdapterLayout::from_document(&doc);
    assert_eq!(layout.combos.len(), 1);
    let combo = &layout.combos[0];
    assert_eq!(
        combo.description,
        r#"sticky "meh" modifiers (Alt + Ctrl + Shift) - TailorKey"#
    );
    assert_eq!(combo.layers, vec![0, 2]);

    let json = layout.to_standard_json()?;
    let rendered = render_standard_template(&json, "{{combos}}\n")?;
    assert!(
        rendered.contains("// sticky \"meh\" modifiers (Alt + Ctrl + Shift) - TailorKey"),
        "combo description rendered as comment"
    );
    assert!(
        rendered.contains("bindings = <&kp A>;"),
        "bindings rendered without extra spacing"
    );
    assert!(
        rendered.contains("layers = <0 2>;"),
        "layers property rendered"
    );
    Ok(())
}

#[test]
fn macro_rendering_avoids_double_quotes_and_spacing() -> Result<(), Box<dyn Error>> {
    let source = r#"
behaviors {
    rgb_ug_status_macro: rgb_ug_status_macro {
        label = "RGB_UG_STATUS";
        compatible = "zmk,behavior-macro";
        #binding-cells = <0>;
        bindings = < &rgb_ug RGB_STATUS >;
    };
};
"#;
    let doc = DtsDocument::parse_str(source)?;
    let layout = AdapterLayout::from_document(&doc);
    let json = layout.to_standard_json()?;
    let rendered = render_standard_template(&json, "{{macros}}\n")?;
    assert!(
        rendered.contains(r#"compatible = "zmk,behavior-macro";"#),
        "compatible property should only have one set of quotes"
    );
    assert!(
        rendered.contains(r#"label = "RGB_UG_STATUS";"#),
        "label property should be preserved"
    );
    assert!(
        rendered.contains("<&rgb_ug RGB_STATUS>;"),
        "bindings should omit extra spaces inside angle brackets"
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
    assert!(rendered.contains("#define LAYER_default_layer 0"));
    assert!(rendered.contains("combo_esc"));
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
    assert!(rendered.contains("/* Second Comment */\n\n#define LAYER_default_layer 0"));
    assert!(rendered.contains("#define LAYER_default_layer 0\n\n/* Footer Comment */"));
    Ok(())
}

#[test]
fn template_supports_loops() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("ast_walker_complex"))?;
    let layout = AdapterLayout::from_document(&doc);
    let json = layout.to_standard_json()?;

    let template = r#"
/ {
    keymap {
        compatible = "zmk,keymap";
{% for layer in layout.layers %}
        {{ layer.name }} {
            bindings = < {{ layer.bindings | join(" ") }} >;
        };
{% endfor %}
    };
};
"#;

    let rendered = render_standard_template(&json, template)?;
    assert!(rendered.contains("default_layer"));
    assert!(rendered.contains("bindings = < &kp A"));
    Ok(())
}

#[test]
fn template_export_strips_metadata_sections_before_parse() -> Result<(), Box<dyn Error>> {
    const TEMPLATE: &str = r#"/* Includes */
{{includes}}

/* Custom Device-tree */
{{custom_devicetree}}

/ {
    keymap {
        compatible = "zmk,keymap";

{{rendered_layers}}
    };
};
"#;

    const RENDERED: &str = r#"/* Includes */
#if defined(TEST) || \
    defined(TEST2)
#error "naming conflict"
#endif

/* Custom Device-tree */
&mmv {
#if defined(FOO) || \
    defined(BAR)
#error "guard"
#endif
};

/ {
    keymap {
        compatible = "zmk,keymap";

        base {
            bindings = < &kp A &kp B >;
        };
    };
};
"#;

    let json = export_standard_str_with_template(RENDERED, TEMPLATE)?;
    let parsed: Value = serde_json::from_str(&json)?;

    let metadata = parsed
        .get("metadata")
        .and_then(|value| value.as_object())
        .expect("metadata captured");
    assert!(
        metadata
            .get("includes")
            .and_then(|value| value.as_str())
            .expect("includes stored")
            .contains("#if defined(TEST) || \\")
    );
    assert!(
        metadata
            .get("custom_devicetree")
            .and_then(|value| value.as_str())
            .expect("custom devicetree stored")
            .contains("#if defined(FOO) || \\")
    );

    let layers = parsed
        .get("layers")
        .and_then(|value| value.as_array())
        .expect("layers present");
    assert_eq!(layers.len(), 1);
    let bindings = layers[0]
        .get("bindings")
        .and_then(|value| value.as_array())
        .expect("bindings array");
    assert_eq!(
        bindings
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["&kp A", "&kp B"]
    );
    Ok(())
}
