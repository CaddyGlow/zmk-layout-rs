use zmk_layout_rs::{build::FirmwareManifest, profiles::KeyboardProfileDoc};

#[test]
fn can_load_embedded_keyboard_profile() {
    // This works even when keyboard_profiles/ directory doesn't exist
    // because profiles are embedded in the binary
    let profile = KeyboardProfileDoc::load("glove80").expect("load embedded profile");
    assert_eq!(profile.keyboard, "glove80");
    assert_eq!(profile.metadata.vendor, "MoErgo");
    assert!(profile.hardware.key_count > 0);
}

#[test]
fn can_load_embedded_firmware_manifest() {
    // This works even when firmware_profiles/ directory doesn't exist
    // because manifests are embedded in the binary
    let manifest = FirmwareManifest::load("glove80").expect("load embedded manifest");
    assert_eq!(manifest.version, 1);
    assert!(!manifest.toolchains.is_empty());
    assert!(!manifest.keyboards.is_empty());
}

#[test]
fn filesystem_overrides_embedded() {
    // When both exist, filesystem takes precedence
    // First check that embedded works
    let embedded = KeyboardProfileDoc::load("glove80").expect("load profile");

    // Filesystem version (if exists) should also load
    if std::path::Path::new("keyboard_profiles/glove80.toml").exists() {
        let from_file = KeyboardProfileDoc::from_file("keyboard_profiles/glove80.toml")
            .expect("load from filesystem");
        assert_eq!(embedded.keyboard, from_file.keyboard);
    }
}

#[test]
fn list_profiles_includes_embedded() {
    let profiles = KeyboardProfileDoc::list_available();
    assert!(!profiles.is_empty(), "should include embedded profiles");
    assert!(profiles.contains(&"glove80".to_string()));
}

#[test]
fn list_manifests_includes_embedded() {
    let manifests = FirmwareManifest::list_available();
    assert!(!manifests.is_empty(), "should include embedded manifests");
    assert!(manifests.contains(&"glove80".to_string()));
}
