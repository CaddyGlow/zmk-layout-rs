use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use tempfile::tempdir;
use zmk_layout_rs::dts::DtsDocument;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from("tests/fixtures").join(name)
}

#[test]
fn cli_apply_writes_output_and_warns_on_mismatch() {
    let dir = tempdir().expect("tempdir");
    let base_src = fixture("cli_base.dts");
    let base_path = dir.path().join("base.dts");
    fs::copy(&base_src, &base_path).expect("copy base");
    let output_path = dir.path().join("updated.dts");

    let mut cmd = cargo_bin_cmd!("zmk-layout");
    cmd.arg("apply")
        .arg("--tasks")
        .arg(fixture("cli_tasks.toml"))
        .arg("--base-layout")
        .arg(&base_path)
        .arg("--base-template")
        .arg("other-template")
        .arg("--base-version")
        .arg("9.9.9")
        .arg("--output")
        .arg(&output_path);

    let assert = cmd
        .assert()
        .success()
        .stdout(predicates::str::contains("APPLIED").count(2))
        .stderr(
            predicates::str::contains("warning: [base].template")
                .and(predicates::str::contains("wrote updated layout")),
        );

    let updated = fs::read_to_string(&output_path).expect("read output");
    assert!(updated.contains("&kp ESC"));
    assert!(updated.contains("combo_demo"));

    drop(assert);
}

#[test]
fn cli_apply_prints_combo_conditions_when_requested() {
    let dir = tempdir().expect("tempdir");
    let base_src = fixture("cli_base.dts");
    let base_path = dir.path().join("base.dts");
    fs::copy(&base_src, &base_path).expect("copy base");

    let mut cmd = cargo_bin_cmd!("zmk-layout");
    cmd.arg("apply")
        .arg("--tasks")
        .arg(fixture("cli_tasks.toml"))
        .arg("--base-layout")
        .arg(&base_path)
        .arg("--combo-conditions");

    cmd.assert()
        .success()
        .stdout(
            predicates::str::contains("combo conditions:").and(predicates::str::contains(
                "combo_demo (combos.combo_demo) :: layer_state == base",
            )),
        );
}

#[test]
fn cli_validate_reports_conflicts() {
    let dir = tempdir().expect("tempdir");
    let base_src = fixture("cli_base.dts");
    let base_path = dir.path().join("base.dts");
    fs::copy(&base_src, &base_path).expect("copy base");

    let mut cmd = cargo_bin_cmd!("zmk-layout");
    cmd.arg("validate")
        .arg("--tasks")
        .arg(fixture("cli_conflict_tasks.toml"))
        .arg("--base-layout")
        .arg(&base_path);

    cmd.assert().failure().stdout(
        predicates::str::contains("[CONFLICT]").and(predicates::str::contains("conflict-check")),
    );
}

#[test]
fn cli_validate_conflicts_can_be_overridden() {
    let dir = tempdir().expect("tempdir");
    let base_src = fixture("cli_base.dts");
    let base_path = dir.path().join("base.dts");
    fs::copy(&base_src, &base_path).expect("copy base");

    let mut cmd = cargo_bin_cmd!("zmk-layout");
    cmd.arg("validate")
        .arg("--tasks")
        .arg(fixture("cli_conflict_tasks.toml"))
        .arg("--base-layout")
        .arg(&base_path)
        .arg("--conflicts")
        .arg("override");

    cmd.assert()
        .success()
        .stdout(predicates::str::contains("conflict overridden"));
}

#[test]
fn cli_diff_prints_patch() {
    let dir = tempdir().expect("tempdir");
    let base_src = fixture("cli_base.dts");
    let base_path = dir.path().join("base.dts");
    fs::copy(&base_src, &base_path).expect("copy base");

    let mut cmd = cargo_bin_cmd!("zmk-layout");
    cmd.arg("diff")
        .arg("--tasks")
        .arg(fixture("cli_tasks.toml"))
        .arg("--base-layout")
        .arg(&base_path);

    cmd.assert()
        .success()
        .stdout(predicates::str::contains("--- ").and(predicates::str::contains("+++ updated")));
}

#[test]
fn cli_firmware_build_prints_request() {
    let dir = tempdir().expect("tempdir");
    let mut cmd = cargo_bin_cmd!("zmk-layout");
    cmd.arg("firmware")
        .arg("build")
        .arg("--manifest")
        .arg(fixture("firmware_manifest.toml"))
        .arg("--keyboard")
        .arg("glove80")
        .arg("--layout-json")
        .arg(fixture("demo_layout.json"))
        .arg("--output-dir")
        .arg(dir.path().join("out"))
        .arg("--dry-run");

    cmd.assert().success().stdout(
        predicates::str::contains("keyboard : glove80 (MoErgo Glove80)")
            .and(predicates::str::contains("vendor   : MoErgo"))
            .and(predicates::str::contains("firmware : v25.05"))
            .and(predicates::str::contains("targets  : left, right")),
    );
}

#[test]
fn cli_profiles_check_validates_profiles() {
    let mut cmd = cargo_bin_cmd!("zmk-layout");
    cmd.arg("profiles")
        .arg("check")
        .arg("keyboard_profiles/glove80.toml");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("[OK ]"));
}

#[test]
fn cli_profiles_check_reports_errors() {
    let mut cmd = cargo_bin_cmd!("zmk-layout");
    cmd.arg("profiles")
        .arg("check")
        .arg(fixture("profiles/bad_profile.toml"));
    cmd.assert().failure().stderr(
        predicates::str::contains("[ERR]").and(predicates::str::contains("keyboard profile")),
    );
}

#[test]
fn cli_profiles_check_all_scans_directory() {
    let mut cmd = cargo_bin_cmd!("zmk-layout");
    cmd.arg("profiles")
        .arg("check")
        .arg("--all")
        .arg("--profiles-dir")
        .arg(fixture("profiles"));
    cmd.assert()
        .failure()
        .stdout(predicates::str::contains("profiles/good_profile.toml"))
        .stderr(predicates::str::contains("profiles/bad_profile.toml"));
}

#[test]
fn cli_layer_round_trip_without_template_placeholders() {
    let dir = tempdir().expect("tempdir");
    let template = fixture("sample_keymap.dtsi");
    let dts_path = dir.path().join("template.dts");
    fs::copy(&template, &dts_path).expect("copy template");
    let json_path = dir.path().join("layout.json");

    let mut export_cmd = cargo_bin_cmd!("zmk-layout");
    export_cmd
        .arg("layer")
        .arg("export")
        .arg("--dts")
        .arg(&dts_path)
        .arg("--json")
        .arg(&json_path);
    export_cmd.assert().success();

    let output_path = dir.path().join("output.dts");
    let mut import_cmd = cargo_bin_cmd!("zmk-layout");
    import_cmd
        .arg("layer")
        .arg("import")
        .arg("--json")
        .arg(&json_path)
        .arg("--template")
        .arg(&template)
        .arg("--output")
        .arg(&output_path);
    import_cmd.assert().success();

    let rendered = fs::read_to_string(&output_path).expect("read output");
    assert!(
        rendered.contains("default_layer"),
        "import should render the layer name"
    );
    assert!(
        rendered.contains("&kp A"),
        "bindings from the exported JSON should remain"
    );
    DtsDocument::parse_str(&rendered).expect("rendered output parses as DTS");
}

#[test]
fn cli_layer_import_export_with_template_placeholders() {
    let dir = tempdir().expect("tempdir");
    let template = fixture("layer_template.j2");
    let json_path = dir.path().join("layout.json");
    let layout = json!({
        "title": "Template Test",
        "layers": [{
            "name": "base",
            "bindings": ["&kp A", "&kp B"]
        }],
        "combos": [{
            "name": "esc_combo",
            "keyPositions": [0, 1],
            "binding": "&kp ESC"
        }]
    });
    fs::write(&json_path, layout.to_string()).expect("write json");

    let output_path = dir.path().join("rendered.dts");
    let mut import_cmd = cargo_bin_cmd!("zmk-layout");
    import_cmd
        .arg("layer")
        .arg("import")
        .arg("--json")
        .arg(&json_path)
        .arg("--template")
        .arg(&template)
        .arg("--output")
        .arg(&output_path);
    import_cmd.assert().success();

    let rendered = fs::read_to_string(&output_path).expect("read rendered");
    assert!(
        rendered.contains("esc_combo"),
        "rendered DTS should include the combo"
    );
    assert!(
        rendered.contains("&kp ESC"),
        "rendered DTS should include combo binding"
    );
    DtsDocument::parse_str(&rendered).expect("rendered output parses as DTS");

    let roundtrip_json = dir.path().join("roundtrip.json");
    let mut export_cmd = cargo_bin_cmd!("zmk-layout");
    export_cmd
        .arg("layer")
        .arg("export")
        .arg("--dts")
        .arg(&output_path)
        .arg("--template")
        .arg(&template)
        .arg("--json")
        .arg(&roundtrip_json);
    export_cmd.assert().success();

    let exported = fs::read_to_string(&roundtrip_json).expect("read roundtrip json");
    let value: Value = serde_json::from_str(&exported).expect("valid roundtrip json");
    let layer_count = value
        .get("layers")
        .and_then(|layers| layers.as_array())
        .map(|layers| layers.len());
    assert_eq!(
        layer_count,
        Some(1),
        "layer count should survive round-trip"
    );
    let combo_binding = value
        .get("combos")
        .and_then(|combos| combos.as_array())
        .and_then(|combos| combos.get(0))
        .and_then(|combo| combo.get("binding"))
        .and_then(|binding| binding.as_str())
        .unwrap_or("");
    assert_eq!(combo_binding, "&kp ESC", "combo binding preserved");
}
