use zmk_layout_rs::{
    dts::DtsDocument,
    providers::KeymapDocument,
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

    let doc = execution.document.document();
    let base_layer = doc
        .items
        .iter()
        .find_map(|item| match item {
            zmk_layout_rs::ast::DtItem::Node(node) if node.name == "keymap" => Some(node),
            _ => None,
        })
        .expect("keymap node");

    // base layer should now start with &kp ESC
    let base_node = base_layer
        .children
        .iter()
        .find_map(|item| match item {
            zmk_layout_rs::ast::DtItem::Node(node) if node.name == "base" => Some(node),
            _ => None,
        })
        .expect("base node");
    let base_bindings = base_node
        .properties
        .iter()
        .find(|prop| prop.name == "bindings")
        .expect("base bindings");
    assert!(base_bindings.value.raw.contains("&kp ESC"));

    // confirm nav bindings replaced
    let nav_node = base_layer
        .children
        .iter()
        .find_map(|item| match item {
            zmk_layout_rs::ast::DtItem::Node(node) if node.name == "nav" => Some(node),
            _ => None,
        })
        .expect("nav node");
    let nav_bindings = nav_node
        .properties
        .iter()
        .find(|prop| prop.name == "bindings")
        .expect("nav bindings");
    assert!(nav_bindings.value.raw.contains("&kp LEFT"));

    // combo added
    let combos_root = doc
        .items
        .iter()
        .find_map(|item| match item {
            zmk_layout_rs::ast::DtItem::Node(node) if node.name == "combos" => Some(node),
            _ => None,
        })
        .expect("combos node");
    assert!(combos_root.children.iter().any(
        |item| matches!(item, zmk_layout_rs::ast::DtItem::Node(node) if node.name == "combo_enter")
    ));

    // nav layer moved before num
    let order: Vec<String> = base_layer
        .children
        .iter()
        .filter_map(|item| match item {
            zmk_layout_rs::ast::DtItem::Node(node)
                if node.properties.iter().any(|prop| prop.name == "bindings") =>
            {
                Some(node.name.clone())
            }
            _ => None,
        })
        .collect();
    let expected = vec!["base".to_string(), "nav".to_string(), "num".to_string()];
    assert_eq!(
        order
            .into_iter()
            .filter(|name| name == "base" || name == "nav" || name == "num")
            .collect::<Vec<_>>(),
        expected
    );
}
