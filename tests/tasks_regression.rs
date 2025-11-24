use zmk_layout_rs::{
    adapters::standard::AdapterLayout,
    dts::DtsDocument,
    keymap::KeymapDocument,
    tasks::{TaskFile, TaskStatus, apply_tasks},
};

#[test]
fn regression_config_applies_all_tasks() {
    let config = include_str!("fixtures/tasks_regression_config.toml");
    let base = include_str!("fixtures/tasks_regression_base.dts");

    let file = TaskFile::from_toml_str(config).expect("parse regression config");
    let dts = DtsDocument::parse_str(base).expect("parse regression base");
    let document = KeymapDocument::from_document(dts);

    let execution = apply_tasks(document, &file);
    assert!(
        execution
            .results
            .iter()
            .all(|result| matches!(result.status, TaskStatus::Applied))
    );

    let layers = &execution.document.layers;
    let base_layer = layers
        .iter()
        .find(|layer| layer.name == "base")
        .expect("base layer");
    assert!(
        base_layer.bindings.iter().any(|b| b.contains("&kp ESC")),
        "base bindings should contain ESC: {:?}",
        base_layer.bindings
    );

    let nav_layer = layers
        .iter()
        .find(|layer| layer.name == "nav")
        .expect("nav layer");
    assert!(
        nav_layer.bindings.iter().any(|b| b.contains("&kp LEFT")),
        "nav bindings should contain LEFT: {:?}",
        nav_layer.bindings
    );

    assert!(
        execution
            .document
            .combos
            .iter()
            .any(|combo| combo.name == "combo_enter"),
        "combo_enter should exist"
    );

    let expected = vec!["base".to_string(), "nav".to_string(), "num".to_string()];
    let order: Vec<String> = layers.iter().map(|layer| layer.name.clone()).collect();
    assert_eq!(order, expected);
}
