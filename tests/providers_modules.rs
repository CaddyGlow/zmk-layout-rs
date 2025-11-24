use zmk_layout_rs::{
    bindings::BindingParser,
    dts::DtsDocument,
    providers::{
        BehaviorProvider, ComboProvider,
        format::{BindingFormat, format_bindings_raw, parse_binding_groups},
    },
};

#[test]
fn binding_format_normalizes_and_rejects_empty() {
    let parser = BindingParser::new();
    let format = BindingFormat::new(&parser);

    let normalized = format.normalize_binding("&kp A").expect("normalize");
    assert_eq!(normalized, "&kp A");

    let err = format
        .normalize_binding("   ")
        .expect_err("empty binding should fail");
    assert!(
        err.to_string().contains("binding string cannot be empty"),
        "unexpected error: {err}"
    );
}

#[test]
fn binding_group_parser_splits_multi_binding_sequences() {
    let groups = parse_binding_groups("< &kp A &kp B >");
    assert_eq!(groups, vec!["&kp A", "&kp B"]);

    let formatted = format_bindings_raw(&groups.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(formatted.trim(), "< &kp A &kp B >");
}

#[test]
fn combo_provider_extracts_description_and_layers() {
    let source = r#"
/ {
    combos {
        // tap dance
        combo_demo {
            key-positions = <0 1>;
            bindings = <&kp ESC>;
            layers = <1 2>;
            timeout-ms = <50>;
        };
    };
};
"#;
    let doc = DtsDocument::parse_str(source).expect("parse");
    let combos = ComboProvider::new(&doc).combos();
    assert_eq!(combos.len(), 1);
    let combo = &combos[0];
    assert_eq!(combo.name, "combo_demo");
    assert_eq!(combo.key_positions, vec![0, 1]);
    assert_eq!(combo.layers, vec![1, 2]);
    assert_eq!(combo.timeout_ms, Some(50));
    assert_eq!(
        combo.description.as_deref(),
        Some("tap dance"),
        "leading comment becomes description"
    );
}

#[test]
fn behavior_provider_collects_metadata() {
    let source = r#"
/ {
    behaviors {
        demo: demo {
            compatible = "zmk,behavior-demo";
            label = "DEMO";
            #binding-cells = <1>;
            bindings = <&kp A>;
            wait-ms = <10>;
            tap-ms = <20>;
        };
    };
};
"#;
    let doc = DtsDocument::parse_str(source).expect("parse");
    let behaviors = BehaviorProvider::new(&doc).behaviors();
    assert_eq!(behaviors.len(), 1);
    let behavior = &behaviors[0];
    assert_eq!(behavior.name, "demo");
    assert_eq!(behavior.compatible.as_deref(), Some("zmk,behavior-demo"));
    assert_eq!(behavior.binding_cells, Some(1));
    assert_eq!(behavior.bindings, vec!["&kp A"]);
    assert_eq!(behavior.wait_ms, Some(10));
    assert_eq!(behavior.tap_ms, Some(20));
    assert_eq!(behavior.label.as_deref(), Some("DEMO"));
}
