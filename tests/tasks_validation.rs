use zmk_layout_rs::{
    dts::DtsDocument,
    providers::KeymapDocument,
    tasks::{TaskConfigError, TaskFile, TaskStatus, apply_tasks},
};

#[test]
fn task_file_requires_format_version() {
    let err = TaskFile::from_toml_str(include_str!("fixtures/tasks_invalid_missing_version.toml"))
        .expect_err("missing format version");
    assert!(matches!(err, TaskConfigError::MissingFormatVersion));
    assert_eq!(err.to_string(), "missing config.format_version");
}

#[test]
fn task_file_rejects_overlapping_targets() {
    let err = TaskFile::from_toml_str(include_str!(
        "fixtures/tasks_invalid_overlapping_targets.toml"
    ))
    .expect_err("overlapping targets");
    assert!(matches!(
        err,
        TaskConfigError::OverlappingTarget { ref new, ref existing }
            if new == "layers.nav.bindings[0]" && existing == "layers.nav"
    ));
    assert_eq!(
        err.to_string(),
        "target 'layers.nav.bindings[0]' overlaps with 'layers.nav'"
    );
}

#[test]
fn task_file_rejects_mismatched_target_path() {
    let err = TaskFile::from_toml_str(include_str!(
        "fixtures/tasks_invalid_mismatched_target.toml"
    ))
    .expect_err("mismatched target");
    assert!(matches!(
        err,
        TaskConfigError::InvalidField { field, .. } if field == "target"
    ));
    assert_eq!(
        err.to_string(),
        "invalid field 'target' for task #0 (Override): override target `combos.wrong` must match path `layers.base.bindings[0]`"
    );
}

#[test]
fn task_expected_state_conflict_reports_mismatch() {
    let file = TaskFile::from_toml_str(include_str!("fixtures/tasks_conflict_expected_state.toml"))
        .expect("parse conflict fixture");
    let base = include_str!("fixtures/tasks_regression_base.dts");
    let dts = DtsDocument::parse_str(base).expect("parse base layout");
    let document = KeymapDocument::from_document(dts);

    let exec = apply_tasks(document, &file);
    assert_eq!(exec.results.len(), 1);
    let outcome = &exec.results[0];
    assert_eq!(outcome.status, TaskStatus::Conflict);
    assert_eq!(
        outcome.message.as_deref(),
        Some("expected `&kp ESC` for target `layers.base.bindings[0]` but found `&kp Q`")
    );
}
