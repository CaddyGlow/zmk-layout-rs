use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::{fs, path::PathBuf};
use tempfile::tempdir;

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
