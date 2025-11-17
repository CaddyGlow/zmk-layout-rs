use std::{error::Error, fs, path::PathBuf};

use zmk_layout_rs::{
    dts::DtsDocument,
    providers::{BehaviorProvider, ComboProvider, KeymapDocument, KeymapProvider},
};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.dts", name));
    fs::read_to_string(path).expect("fixture exists")
}

#[test]
fn provider_reads_layer_bindings() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("dts_roundtrip_keymap"))?;
    let provider = KeymapProvider::new(doc);
    let bindings = provider.bindings_for_layer("default_layer")?;
    assert_eq!(bindings.len(), 4);
    assert!(bindings[0].to_binding_string().contains("&kp"));
    Ok(())
}

#[test]
fn provider_can_set_binding() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("dts_roundtrip_keymap"))?;
    let mut provider = KeymapProvider::new(doc);
    provider.set_binding("default_layer", 1, "&kp ESC")?;
    let updated = provider.into_document().to_string()?;
    assert!(updated.contains("&kp ESC"));
    Ok(())
}

#[test]
fn behavior_provider_lists_behaviors() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("providers_behaviors"))?;
    let provider = BehaviorProvider::new(&doc);
    let behaviors = provider.behaviors();
    assert_eq!(behaviors.len(), 2);
    assert!(behaviors.iter().any(|b| b.name == "ht"));
    assert_eq!(behaviors[0].bindings.len(), 2);
    Ok(())
}

#[test]
fn combo_provider_lists_combos() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("ast_walker_complex"))?;
    let combos = ComboProvider::new(&doc).combos();
    assert_eq!(combos.len(), 1);
    assert_eq!(combos[0].name, "combo_esc");
    assert_eq!(combos[0].key_positions, vec![0, 1]);
    Ok(())
}

#[test]
fn provider_updates_combo_properties() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("ast_walker_complex"))?;
    let mut provider = KeymapProvider::new(doc);
    provider.set_combo_bindings("combo_esc", &["&kp SPACE"])?;
    provider.set_combo_key_positions("combo_esc", &[2, 3])?;
    provider.set_combo_timeout_ms("combo_esc", Some(75))?;
    let intermediate = provider.document().to_string()?;
    assert!(intermediate.contains("timeout-ms = < 75 >;"));
    provider.set_combo_timeout_ms("combo_esc", None)?;
    let updated = provider.into_document().to_string()?;
    assert!(updated.contains("bindings = < &kp SPACE >;"));
    assert!(updated.contains("key-positions = < 2 3 >;"));
    assert!(!updated.contains(">;;"));
    assert!(!updated.contains("timeout-ms ="));
    Ok(())
}

#[test]
fn provider_updates_behavior_bindings() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("providers_behaviors"))?;
    let mut provider = KeymapProvider::new(doc);
    provider.set_behavior_bindings("ht", &["&kp Z", "&kp X"])?;
    let updated = provider.into_document().to_string()?;
    assert!(updated.contains("bindings = < &kp Z &kp X >;"));
    Ok(())
}

#[test]
fn provider_sets_layer_bindings() -> Result<(), Box<dyn Error>> {
    let doc = DtsDocument::parse_str(&fixture("dts_roundtrip_keymap"))?;
    let mut provider = KeymapProvider::new(doc);
    assert_eq!(provider.layer_names(), vec!["default_layer".to_string()]);
    provider.set_layer_bindings("default_layer", &["&kp X", "&kp Y"])?;
    let updated = provider.into_document().to_string()?;
    assert!(updated.contains("&kp X &kp Y"));
    Ok(())
}

#[test]
fn provider_creates_missing_layers_when_needed() -> Result<(), Box<dyn Error>> {
    let template = "keymap {\n};\n";
    let doc = DtsDocument::parse_str(template)?;
    let mut provider = KeymapProvider::new(doc);
    provider.set_layer_bindings("new_layer", &["&kp A", "&kp B"])?;
    let updated = provider.into_document().to_string()?;
    assert!(updated.contains("new_layer"));
    assert!(updated.contains("&kp A &kp B"));
    Ok(())
}

#[test]
fn keymap_document_round_trip() -> Result<(), Box<dyn Error>> {
    let mut keymap = KeymapDocument::parse_str(&fixture("dts_roundtrip_keymap"))?;
    assert_eq!(keymap.behaviors().len(), 0);
    keymap.set_binding("default_layer", 0, "&kp Z")?;
    let output = keymap.document().to_string()?;
    assert!(output.contains("&kp Z"));
    Ok(())
}
