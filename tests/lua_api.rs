use std::{cell::RefCell, fs, rc::Rc};

use mlua::Lua;
use tempfile::tempdir;

use zmk_layout_rs::{
    adapters::standard::export_standard_str,
    dts::DtsDocument,
    layout_engine::LayoutEngine, lua_api::api::install_layout_api, providers::KeymapDocument,
};

fn make_engine() -> LayoutEngine {
    let doc = KeymapDocument::parse_str(include_str!("fixtures/sample_keymap.dtsi")).unwrap();
    LayoutEngine::new(doc)
}

fn with_lua<F>(engine: Rc<RefCell<LayoutEngine>>, f: F)
where
    F: FnOnce(&Lua) -> (),
{
    let lua = Lua::new();
    install_layout_api(&lua, Rc::clone(&engine), Rc::new(RefCell::new(Vec::new()))).unwrap();
    f(&lua);
}

#[test]
fn layer_bind_updates_are_1_based() {
    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        lua.load(
            r#"
            layout:layer("default_layer")
                :bind(1, "&kp B")
                :apply()
            "#,
        )
        .exec()
        .unwrap();
    });

    let bindings = engine.borrow().layer_bindings("default_layer").unwrap();
    assert_eq!(bindings[0], "&kp B");
}

#[test]
fn combo_auto_applies_and_sets_binding() {
    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        lua.load(
            r#"
            local esc = layout:combo("esc"):keys({1, 2}):binding("&kp ESC")
            layout:layer("default_layer")
                :bind(1, esc)
                :apply()
            "#,
        )
        .exec()
        .unwrap();
    });

    let binding = engine.borrow().layer_bindings("default_layer").unwrap()[0].clone();
    assert_eq!(binding, "&kp ESC");
    assert!(engine.borrow().combo_to_string("esc").is_some());
}

#[test]
fn query_snapshots_are_read_only() {
    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        let err = lua
            .load(
                r#"
                local info = layout:get_layer("default_layer")
                local bindings = info:bindings()
                bindings[1] = "&kp C"
                "#,
            )
            .exec()
            .unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("read-only"),
            "expected read-only error, got {message}"
        );
    });

    let bindings = engine.borrow().layer_bindings("default_layer").unwrap();
    assert_eq!(bindings[0], "&kp A", "original binding remains unchanged");
}

#[test]
fn invalid_index_is_rejected() {
    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        let err = lua
            .load(
                r#"
                layout:layer("default_layer")
                    :bind(0, "&kp B")
                    :apply()
                "#,
            )
            .exec()
            .unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(">= 1"),
            "expected 1-based index error, got {message}"
        );
    });
}

#[test]
fn load_and_save_dtsi_round_trip() {
    let dir = tempdir().unwrap();
    let input_path = dir.path().join("input.dts");
    let output_path = dir.path().join("output.dts");
    fs::write(
        &input_path,
        r#"
keymap {
    compatible = "zmk,keymap";
    default_layer {
        bindings = < &kp A >;
    };
};"#,
    )
    .unwrap();

    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        lua.load(format!(
            r#"
            layout:load_dtsi("{in_path}")
            layout:layer("default_layer")
                :bind(1, "&kp B")
                :apply()
            layout:save_dtsi("{out_path}")
            "#,
            in_path = input_path.display(),
            out_path = output_path.display()
        ))
        .exec()
        .unwrap();
    });

    let saved = fs::read_to_string(&output_path).unwrap();
    assert!(
        saved.contains("&kp B"),
        "expected saved DTS to include updated binding"
    );
}

#[test]
fn parse_dts_from_string() {
    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        lua.load(
            r#"
            local source = [[
            keymap {
                compatible = "zmk,keymap";
                default_layer {
                    bindings = < &kp C >;
                };
            };
            ]]
            layout:parse_dts(source)
            "#,
        )
        .exec()
        .unwrap();
    });

    let bindings = engine.borrow().layer_bindings("default_layer").unwrap();
    assert_eq!(bindings[0], "&kp C");
}

#[test]
fn load_and_save_json_round_trip() {
    let dir = tempdir().unwrap();
    let template_path = dir.path().join("template.dts");
    let json_path = dir.path().join("layout.json");
    let output_json = dir.path().join("updated.json");

    let template = r#"
keymap {
    compatible = "zmk,keymap";
    default_layer {
        bindings = < &kp A >;
    };
};"#;
    fs::write(&template_path, template).unwrap();

    // Export minimal JSON from the template to seed the import.
    let doc = DtsDocument::parse_str(template).unwrap();
    let json = export_standard_str(&doc).unwrap();
    fs::write(&json_path, json).unwrap();

    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        lua.load(format!(
            r#"
            layout:load_json("{json_path}", "{template_path}")
            layout:layer("default_layer")
                :bind(1, "&kp B")
                :apply()
            layout:save_json("{output_json}")
            "#,
            json_path = json_path.display(),
            template_path = template_path.display(),
            output_json = output_json.display()
        ))
        .exec()
        .unwrap();
    });

    let saved = fs::read_to_string(&output_json).unwrap();
    assert!(
        saved.contains("&kp B"),
        "expected saved JSON to include updated binding"
    );
}
