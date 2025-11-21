use std::{fs, io::Write};

use serde_json::Value;
use tempfile::NamedTempFile;
use zmk_layout_rs::adapters::{
    bundle::{BundleOutput, BundleOverlays, BundleTarget, LayoutBundle},
    standard::{AdapterLayout, LayerSpec, LayoutMetadata},
};

#[test]
fn moergo_bundle_imports() {
    let json = fs::read_to_string(
        "examples/85f92852-413b-4931-ac7d-cf42e6b129eb_TailorKey v4.2h Bilateral.json",
    )
    .expect("fixture present");
    let bundle = LayoutBundle::from_moergo_str(&json).expect("import");

    assert_eq!(bundle.format_version, "layout-bundle/2025-02-01");
    assert_eq!(bundle.targets.len(), 1);
    assert_eq!(bundle.targets[0].id, "moergo");
    assert!(!bundle.layout.layers.is_empty());
    assert!(
        bundle
            .layout
            .layers
            .iter()
            .all(|layer| !layer.bindings.is_empty())
    );
    assert_eq!(
        bundle.metadata.keyboard.as_deref(),
        Some("glove80"),
        "keyboard should be captured from MoErgo payload"
    );
}

#[test]
fn moergo_bundle_exports() {
    let json = fs::read_to_string(
        "examples/85f92852-413b-4931-ac7d-cf42e6b129eb_TailorKey v4.2h Bilateral.json",
    )
    .expect("fixture present");
    let bundle = LayoutBundle::from_moergo_str(&json).expect("import");
    let exported = bundle.to_moergo_json().expect("export");
    let value: Value = serde_json::from_str(&exported).expect("json");

    assert_eq!(
        value["layer_names"].as_array().map(|layers| layers.len()),
        Some(bundle.layout.layers.len())
    );
    assert_eq!(
        value["combos"].as_array().map(|combos| combos.len()),
        Some(bundle.layout.combos.len())
    );
}

#[test]
fn moergo_config_parameters_become_defines() {
    let json = fs::read_to_string("examples/test.json").expect("fixture present");
    let bundle = LayoutBundle::from_moergo_str(&json).expect("import");

    assert_eq!(
        bundle.symbols.defines.get("HID_POINTING"),
        Some(&Value::String("y".into()))
    );
    assert_eq!(
        bundle.symbols.defines.get("HID_POINTING_SMOOTH_SCROLLING"),
        Some(&Value::String("y".into()))
    );

    let target = bundle.targets.first().expect("target present");
    assert!(
        target.defines.contains(&"HID_POINTING".into())
            && target
                .defines
                .contains(&"HID_POINTING_SMOOTH_SCROLLING".into())
    );
    assert_eq!(
        target.defines.len(),
        bundle.symbols.defines.len(),
        "all defines should be attached to the default target"
    );
}

#[test]
fn renders_target_scoped_overlays_and_defines() {
    let mut template = NamedTempFile::new().expect("temp template");
    writeln!(
        template,
        r#"keymap {{
    compatible = "zmk,keymap";
}};

/* includes:
{{{{ resolved_includes }}}}
*/
/* defines:
{{{{ defines }}}}
*/
/* dt: {{{{ custom_devicetree }}}} */
/* behaviors: {{{{ custom_defined_behaviors }}}} */
/* macros: {{{{ custom_defined_macros }}}} */
/* fragment: {{{{ key_position_header }}}} */
"#
    )
    .expect("template write");

    let mut bundle = LayoutBundle::default();
    bundle.layout = AdapterLayout {
        layers: vec![LayerSpec {
            name: "BASE".into(),
            bindings: vec!["&kp A".into()],
        }],
        combos: vec![],
        behaviors: vec![],
        macros: vec![],
        input_listeners: vec![],
        metadata: LayoutMetadata::default(),
    };
    bundle.overlays = BundleOverlays {
        custom_devicetree: Some("OVERLAY_DT".into()),
        custom_behaviors: Some("OVERLAY_BEH".into()),
        custom_macros: Some("OVERLAY_MAC".into()),
        input_listeners: None,
        fragments: [("key_position_header".into(), "HEADER".into())]
            .into_iter()
            .collect(),
    };
    bundle.symbols.includes = vec!["<base.h>".into()];
    bundle
        .symbols
        .defines
        .insert("FEATURE".into(), Value::String("1".into()));

    let template_path = template.path().to_string_lossy().into_owned();
    let full_overlays = vec![
        "custom_devicetree".into(),
        "custom_behaviors".into(),
        "custom_macros".into(),
        "fragments.key_position_header".into(),
    ];

    bundle.targets = vec![
        BundleTarget {
            id: "full".into(),
            template: Some(template_path.clone()),
            overlays: full_overlays,
            defines: vec!["FEATURE".into()],
            includes: vec!["<extra.h>".into()],
            output: Some(BundleOutput {
                format: "dtsi".into(),
            }),
            ..BundleTarget::default()
        },
        BundleTarget {
            id: "minimal".into(),
            template: Some(template_path),
            overlays: vec![],
            defines: vec![],
            includes: vec![],
            output: Some(BundleOutput {
                format: "dtsi".into(),
            }),
            ..BundleTarget::default()
        },
    ];

    bundle.validate().expect("valid bundle");

    let rendered_full = bundle.render_target("full", None).expect("render full");
    assert!(
        rendered_full.contains("OVERLAY_DT"),
        "rendered:\n{rendered_full}"
    );
    assert!(rendered_full.contains("OVERLAY_BEH"));
    assert!(rendered_full.contains("OVERLAY_MAC"));
    assert!(rendered_full.contains("HEADER"));
    assert!(rendered_full.contains("<base.h>"));
    assert!(rendered_full.contains("<extra.h>"));
    assert!(rendered_full.contains("#define FEATURE 1"));

    let rendered_minimal = bundle
        .render_target("minimal", None)
        .expect("render minimal");
    assert!(!rendered_minimal.contains("OVERLAY_DT"));
    assert!(!rendered_minimal.contains("OVERLAY_BEH"));
    assert!(!rendered_minimal.contains("OVERLAY_MAC"));
    assert!(!rendered_minimal.contains("HEADER"));
    assert!(rendered_minimal.contains("<base.h>"));
    assert!(!rendered_minimal.contains("<extra.h>"));
    assert!(!rendered_minimal.contains("#define FEATURE 1"));
}

#[test]
fn validate_catches_missing_template_and_defines() {
    let mut bundle = LayoutBundle::default();
    bundle.targets = vec![BundleTarget {
        id: "missing_template".into(),
        ..BundleTarget::default()
    }];
    let err = bundle.validate().expect_err("missing template should fail");
    assert!(err
        .to_string()
        .contains("missing a template path"), "got {err:?}");

    let mut bundle = LayoutBundle::default();
    bundle
        .symbols
        .defines
        .insert("KNOWN".into(), Value::Bool(true));
    bundle.targets = vec![BundleTarget {
        id: "bad_defines".into(),
        template: Some("templates/glove80/keymap.dtsi.j2".into()),
        defines: vec!["UNKNOWN".into()],
        output: Some(BundleOutput {
            format: "dtsi".into(),
        }),
        ..BundleTarget::default()
    }];
    let err = bundle.validate().expect_err("unknown define should fail");
    assert!(
        err.to_string().contains("unknown define `UNKNOWN`"),
        "got {err:?}"
    );
}
