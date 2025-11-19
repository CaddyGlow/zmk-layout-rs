use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tempfile::tempdir;
use zmk_layout_rs::build::{
    BuildError, BuildRequestError, CliDockerBackend, DockerBackend, DockerBuildOptions,
    DockerInvocation, FirmwareBuilder, FirmwareManifest, ManifestError, ProcessStatus,
};

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
    let builder = FirmwareBuilder::new(manifest, Box::new(CliDockerBackend::new()));
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
    let builder = FirmwareBuilder::new(manifest, Box::new(CliDockerBackend::new()));
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

#[test]
fn firmware_builder_runs_moergo_toolchain() {
    let manifest = FirmwareManifest::from_toml_str(include_str!("fixtures/firmware_manifest.toml"))
        .expect("manifest");
    let docker = FakeDockerBackend::new();
    docker.set_on_run(|invocation| {
        let workspace = invocation
            .volumes
            .iter()
            .find(|mount| mount.container_path == PathBuf::from("/workspace"))
            .expect("workspace mount")
            .host_path
            .clone();
        let artifacts = workspace.join("artifacts");
        fs::create_dir_all(&artifacts).expect("artifact dir");
        fs::write(artifacts.join("left.uf2"), b"demo").expect("artifact");
    });
    let builder = FirmwareBuilder::new(manifest, Box::new(docker.clone()));
    let output_dir = tempdir().expect("tempdir");
    let request = builder
        .builder()
        .keyboard("glove80")
        .target("left")
        .layout_json_path(fixture("demo_layout.json"))
        .output_dir(output_dir.path().to_path_buf())
        .build()
        .expect("request");
    let report = builder.build(request).expect("build");
    assert!(report.success);
    assert!(output_dir.path().join("left.uf2").exists());

    let invocations = docker.invocations();
    assert_eq!(invocations.len(), 1);
    let record = &invocations[0];
    assert_eq!(record.image, "ghcr.io/moergo/custom");
    assert_eq!(
        record.env.get("MOERGO_BOARD").map(String::as_str),
        Some("nice_nano_v2")
    );
    assert_eq!(
        record.env.get("MOERGO_SHIELD").map(String::as_str),
        Some("glove80_left")
    );
}

#[test]
fn firmware_builder_runs_zmk_toolchain() {
    let manifest = FirmwareManifest::from_toml_str(include_str!("fixtures/firmware_manifest.toml"))
        .expect("manifest");
    let docker = FakeDockerBackend::new();
    docker.set_on_run(|invocation| {
        let workspace = invocation
            .volumes
            .iter()
            .find(|mount| mount.container_path == PathBuf::from("/workspace"))
            .expect("workspace mount")
            .host_path
            .clone();
        let build_dir = workspace.join("build/right/zephyr");
        fs::create_dir_all(&build_dir).expect("build dir");
        fs::write(build_dir.join("firmware.uf2"), b"demo").expect("artifact");
    });
    let builder = FirmwareBuilder::new(manifest, Box::new(docker.clone()));
    let output_dir = tempdir().expect("tempdir");
    let request = builder
        .builder()
        .keyboard("glove80")
        .toolchain("zmk")
        .target("right")
        .layout_files(fixture("sample_keymap.dtsi"), fixture("sample_config.dtsi"))
        .output_dir(output_dir.path().to_path_buf())
        .build()
        .expect("request");
    let report = builder.build(request).expect("build");
    assert!(report.success);
    assert!(output_dir.path().join("firmware.uf2").exists());

    let invocations = docker.invocations();
    assert_eq!(invocations.len(), 1);
    let record = &invocations[0];
    assert_eq!(record.image, "zmkfirmware/zmk-build-arm");
    assert!(
        record
            .command
            .iter()
            .any(|arg| arg.contains("-DSHIELD=glove80_right"))
    );
    assert_eq!(
        record.env.get("ZMK_CONFIG").map(String::as_str),
        Some("/workspace/config")
    );
}

#[derive(Clone)]
struct FakeDockerBackend {
    inner: Arc<FakeDockerInner>,
}

struct FakeDockerInner {
    invocations: Mutex<Vec<RecordedInvocation>>,
    on_run: Mutex<Option<Box<dyn Fn(&DockerInvocation) + Send + 'static>>>,
}

impl FakeDockerBackend {
    fn new() -> Self {
        Self {
            inner: Arc::new(FakeDockerInner {
                invocations: Mutex::new(Vec::new()),
                on_run: Mutex::new(None),
            }),
        }
    }

    fn set_on_run<F>(&self, handler: F)
    where
        F: Fn(&DockerInvocation) + Send + 'static,
    {
        *self.inner.on_run.lock().expect("lock on_run") = Some(Box::new(handler));
    }

    fn invocations(&self) -> Vec<RecordedInvocation> {
        self.inner
            .invocations
            .lock()
            .expect("lock invocations")
            .clone()
    }
}

#[derive(Clone)]
struct RecordedInvocation {
    image: String,
    command: Vec<String>,
    env: BTreeMap<String, String>,
}

impl RecordedInvocation {
    fn from_invocation(invocation: &DockerInvocation) -> Self {
        Self {
            image: invocation.image.clone(),
            command: invocation.command.clone(),
            env: invocation.env.clone(),
        }
    }
}

impl DockerBackend for FakeDockerBackend {
    fn ensure_available(&self) -> Result<(), BuildError> {
        Ok(())
    }

    fn run(&self, invocation: DockerInvocation) -> Result<ProcessStatus, BuildError> {
        if let Some(handler) = self.inner.on_run.lock().expect("lock handler").as_ref() {
            handler(&invocation);
        }
        let record = RecordedInvocation::from_invocation(&invocation);
        self.inner
            .invocations
            .lock()
            .expect("lock invocations")
            .push(record);
        Ok(ProcessStatus { code: 0 })
    }

    fn build(&self, _opts: DockerBuildOptions) -> Result<(), BuildError> {
        Ok(())
    }
}
