use std::path::PathBuf;

use zmk_layout_rs::build::{BuildRequestError, FirmwareBuilder, FirmwareManifest, ManifestError};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from("tests/fixtures").join(name)
}

#[test]
fn manifest_fixture_loads_profiles() {
    let path = fixture("firmware_manifest.toml");
    let manifest = FirmwareManifest::from_file(&path).expect("manifest");
    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.toolchains.len(), 2);
    assert_eq!(manifest.keyboards.len(), 1);

    let keyboard = manifest.keyboards.get("glove80").expect("keyboard profile");
    assert_eq!(keyboard.targets.len(), 2);
    assert_eq!(keyboard.default_toolchain, "moergo");

    let target = keyboard.targets.iter().find(|t| t.id == "left").unwrap();
    assert_eq!(target.board, "nice_nano_v2");
    assert_eq!(
        target.cmake_defs.get("CONFIG_ZMK_KEYS_PER_SCAN"),
        Some(&"4".into())
    );
}

#[test]
fn build_request_defaults_to_all_targets() {
    let text = include_str!("fixtures/firmware_manifest.toml");
    let manifest = FirmwareManifest::from_toml_str(text).expect("manifest");
    let builder = FirmwareBuilder::new(
        manifest,
        Box::new(zmk_layout_rs::build::CliDockerBackend::new()),
    );
    let request = builder
        .builder()
        .keyboard("glove80")
        .layout_json_path(PathBuf::from("layout.json"))
        .output_dir(PathBuf::from("build"))
        .build()
        .expect("request");
    assert_eq!(request.targets.len(), 2);
    assert!(request.toolchain_id.is_none());
}

#[test]
fn build_request_errors_on_missing_layout() {
    let text = include_str!("fixtures/firmware_manifest.toml");
    let manifest = FirmwareManifest::from_toml_str(text).expect("manifest");
    let builder = FirmwareBuilder::new(
        manifest,
        Box::new(zmk_layout_rs::build::CliDockerBackend::new()),
    );
    let err = builder
        .builder()
        .keyboard("glove80")
        .output_dir(PathBuf::from("build"))
        .build()
        .expect_err("missing layout");
    assert!(matches!(err, BuildRequestError::MissingLayout));
}

#[test]
fn manifest_rejects_missing_default_toolchain() {
    let text = r#"
version = 1

[toolchains.moergo]
kind = "moergo"
image = "demo"

[keyboards.test]
default_toolchain = "missing"
"#;
    let err = FirmwareManifest::from_toml_str(text).expect_err("missing toolchain");
    match err {
        ManifestError::MissingDefaultToolchain(keyboard, toolchain) => {
            assert_eq!(keyboard, "test");
            assert_eq!(toolchain, "missing");
        }
        other => panic!("unexpected error {other:?}"),
    }
}
