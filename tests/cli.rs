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
