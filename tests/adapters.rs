use std::{error::Error, fs, path::PathBuf};

use serde_json::json;
use zmk_layout_rs::{
    adapters::{
        AdapterLayout, export_standard_file, export_standard_str, import_standard_file,
        import_standard_str,
    },
    dts::DtsDocument,
    providers::KeymapProvider,
};

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
