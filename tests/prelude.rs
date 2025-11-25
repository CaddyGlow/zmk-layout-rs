use std::any::type_name;

use zmk_layout_rs::prelude::*;

#[test]
fn prelude_exports_common_aliases() {
    let _provider = KeymapProvider::new(DtsDocument::from_items(Vec::new()));

    assert_eq!(ExecutionMode::Apply, ExecutionMode::Apply);
    assert!(type_name::<DtsKeymapDocument>().contains("KeymapDocument"));
    assert!(type_name::<StandardKeymapDocument>().contains("KeymapDocument"));
    assert!(type_name::<TaskOutcome>().contains("TaskOutcome"));
}
