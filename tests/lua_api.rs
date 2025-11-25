use std::{cell::RefCell, fs, rc::Rc};

use mlua::Lua;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

use zmk_layout_rs::{
    adapters::standard::export_standard_str, dts::DtsDocument, keymap::KeymapDocument,
    layout_engine::LayoutEngine, lua_api::api::install_layout_api,
};

fn make_engine() -> LayoutEngine {
    let dts =
        DtsDocument::parse_str(include_str!("fixtures/sample_keymap.dtsi")).expect("parse dts");
    let adapter = zmk_layout_rs::adapters::standard::AdapterLayout::from_document(&dts);
    LayoutEngine::new(KeymapDocument::from(adapter))
}

fn make_engine_from_str(source: &str) -> LayoutEngine {
    let doc = KeymapDocument::parse_str(source).unwrap();
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

#[test]
fn combo_edits_preserve_existing_fields() {
    let source = r#"
keymap {
    compatible = "zmk,keymap";
    default_layer {
        bindings = < &kp A &kp B >;
    };
    layer_1 {
        bindings = < &kp C &kp D >;
    };
};

combos {
    // zmk-task:condition COND_ACTIVE
    esc_combo {
        key-positions = <0 1>;
        bindings = <&kp ESC>;
        timeout-ms = <50>;
        layers = <1>;
    };
};
"#;

    let engine = Rc::new(RefCell::new(make_engine_from_str(source)));
    with_lua(Rc::clone(&engine), |lua| {
        lua.load(
            r#"
            layout:combo("esc_combo")
                :timeout(100)
                :apply()
            "#,
        )
        .exec()
        .unwrap();
    });

    let (def, combo_text) = {
        let engine_ref = engine.borrow();
        let def = engine_ref
            .document()
            .combos
            .iter()
            .find(|combo| combo.name == "esc_combo")
            .expect("combo is present");
        let combo_text = engine_ref.combo_to_string("esc_combo").unwrap();
        (def.clone(), combo_text)
    };

    assert_eq!(def.key_positions, vec![0, 1], "keys preserved after edit");
    assert_eq!(def.timeout_ms, Some(100), "timeout updated");
    assert_eq!(def.layers, vec![1], "layers preserved");
    assert_eq!(def.binding.as_deref(), Some("&kp ESC"), "binding preserved");
    assert!(
        combo_text.contains("conditions=COND_ACTIVE"),
        "conditions preserved: {combo_text}"
    );
}

#[test]
fn double_apply_throws_error() {
    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        let err = lua
            .load(
                r#"
                local combo = layout:combo("reapply")
                    :keys({1})
                    :binding("&kp A")
                    :apply()
                combo:apply()
                "#,
            )
            .exec()
            .unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("already applied"),
            "expected double-apply error, got {message}"
        );
    });
}

#[test]
fn dts_and_json_string_helpers_work() {
    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        let dts: String = lua
            .load(
                r#"
                layout:parse_dts([[
                keymap {
                    compatible = "zmk,keymap";
                    base { bindings = < &kp A &kp B >; };
                };
                ]])
                layout:layer("base"):bind(2, "&kp C"):apply()
                return layout:to_dts_string()
                "#,
            )
            .eval()
            .unwrap();
        assert!(
            dts.contains("&kp C"),
            "to_dts_string should include edited binding"
        );

        let json: String = lua
            .load(r#"return layout:to_json_string()"#)
            .eval()
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&json).expect("valid JSON");
        let layers = parsed
            .get("layers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(layers.len(), 1, "expected one layer in exported JSON");
        let bindings = layers[0]
            .get("bindings")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            bindings.get(1).and_then(|v| v.as_str()).unwrap_or_default(),
            "&kp C",
            "exported JSON reflects updated bindings"
        );
    });
}

#[test]
fn parse_json_with_template_updates_layout() {
    let dir = tempdir().unwrap();
    let template_path = dir.path().join("template.dts");
    fs::write(
        &template_path,
        r#"
keymap {
    compatible = "zmk,keymap";
    base {
        bindings = < &none >;
    };
};
"#,
    )
    .unwrap();

    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        lua.load(format!(
            r#"
            layout:parse_dts([[
            keymap {{
                compatible = "zmk,keymap";
                base {{ bindings = < &kp A &kp B >; }};
            }};
            ]])
            local json = layout:to_json_string()
            layout:parse_json(json, "{template}")
            "#,
            template = template_path.display()
        ))
        .exec()
        .unwrap();
    });

    let bindings = engine.borrow().layer_bindings("base").unwrap();
    assert_eq!(bindings[0], "&kp A");
    assert_eq!(bindings[1], "&kp B");
}

#[test]
fn firmware_build_dry_run_skips_execution() {
    let dir = tempdir().unwrap();
    let layout_path = dir.path().join("layout.dts");
    let output_dir = dir.path().join("out");
    fs::write(
        &layout_path,
        r#"
keymap {
    compatible = "zmk,keymap";
    base { bindings = < &kp A >; };
};
"#,
    )
    .unwrap();

    let manifest = std::path::Path::new("profiles/firmwares/glove80.toml");
    assert!(
        manifest.exists(),
        "fixture manifest should exist: {}",
        manifest.display()
    );

    let engine = Rc::new(RefCell::new(make_engine()));
    with_lua(Rc::clone(&engine), |lua| {
        let result: mlua::Table = lua
            .load(format!(
                r#"
                return layout:build_firmware({{
                    manifest = "{manifest}",
                    keyboard = "glove80",
                    layout_dts = "{layout}",
                    targets = {{"left"}},
                    output_dir = "{out}",
                    dry_run = true,
                    disable_cache = true,
                }})
                "#,
                manifest = manifest.display(),
                layout = layout_path.display(),
                out = output_dir.display(),
            ))
            .eval()
            .unwrap();

        let built: bool = result.get("built").unwrap();
        assert!(!built, "dry-run should skip actual build");
        let request: mlua::Table = result.get("request").unwrap();
        let layout_kind: String = request.get("layout_kind").unwrap();
        assert_eq!(layout_kind, "layout_dts_path");
        let targets: mlua::Table = request.get("targets").unwrap();
        let first_target: String = targets.get(1).unwrap();
        assert_eq!(first_target, "left");
    });
}
