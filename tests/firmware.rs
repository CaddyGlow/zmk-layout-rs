use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde_json::Value;

use tempfile::tempdir;
use zmk_layout_rs::build::{
    BuildError, BuildRequestError, CliDockerBackend, DockerBackend, DockerBuildOptions,
    DockerInvocation, FirmwareBuilder, FirmwareManifest, LogLevel, ManifestError, ProcessStatus,
    ProgressReporter, WorkspaceManager,
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
    let doc = keyboard.profile.as_ref().expect("keyboard profile doc");
    assert!(
        doc.path
            .display()
            .to_string()
            .ends_with("profiles/keyboards/glove80.toml"),
        "expected profile path to reference glove80 profile"
    );
    assert_eq!(doc.document.metadata.vendor, "MoErgo");

    let target = keyboard.targets.iter().find(|t| t.id == "left").unwrap();
    assert_eq!(target.board, "nice_nano_v2");
    assert_eq!(
        target.cmake_defs.get("CONFIG_ZMK_KEYS_PER_SCAN"),
        Some(&"4".into())
    );
}

#[test]
fn manifest_skips_legacy_yaml_profiles() {
    let text = r#"
version = 1

[toolchains.moergo]
kind = "moergo"
image = "demo"

[keyboards.demo]
default_toolchain = "moergo"

[keyboards.demo.metadata]
profile = "legacy.yaml"
"#;
    let manifest = FirmwareManifest::from_toml_str(text).expect("manifest");
    let keyboard = manifest.keyboards.get("demo").expect("keyboard");
    assert!(
        keyboard.profile.is_none(),
        "yaml profiles should be skipped"
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
        let workspace = host_workspace(invocation);
        let artifacts = workspace.join("artifacts");
        fs::create_dir_all(&artifacts).expect("artifact dir");
        fs::write(artifacts.join("left.uf2"), b"demo").expect("artifact");
        assert!(
            workspace.join("config/nice_nano_v2.keymap").exists(),
            "keymap should be staged for the MoErgo toolchain"
        );
        assert!(
            workspace.join("config/nice_nano_v2.conf").exists(),
            "kconfig should be staged for the MoErgo toolchain"
        );
        let config_text =
            fs::read_to_string(workspace.join("config/nice_nano_v2.conf")).expect("config text");
        assert!(
            config_text.contains("CONFIG_TEST_OVERRIDE=123"),
            "kconfig definitions from the CLI should be appended"
        );
    });
    let builder = FirmwareBuilder::new(manifest, Box::new(docker.clone()));
    let output_dir = tempdir().expect("tempdir");
    let request = builder
        .builder()
        .keyboard("glove80")
        .target("left")
        .kconfig_def("CONFIG_TEST_OVERRIDE", "123")
        .layout_files(fixture("cli_base.dts"), Some(fixture("sample_config.dtsi")))
        .output_dir(output_dir.path().to_path_buf())
        .build()
        .expect("request");
    let report = builder.build(request).expect("build");
    assert!(report.success);
    assert!(output_dir.path().join("left.uf2").exists());
    assert_eq!(
        report.metadata.entries.get("keyboard").map(String::as_str),
        Some("glove80")
    );
    assert_eq!(
        report.metadata.entries.get("toolchain").map(String::as_str),
        Some("moergo")
    );
    assert_eq!(
        report
            .metadata
            .entries
            .get("targets.completed")
            .map(String::as_str),
        Some("left")
    );
    assert_eq!(
        report
            .metadata
            .entries
            .get("artifacts.count")
            .map(String::as_str),
        Some("1")
    );
    let left_artifacts = report
        .artifacts
        .per_target
        .get("left")
        .expect("left artifacts");
    assert_eq!(left_artifacts.len(), 1);
    assert_eq!(left_artifacts[0], output_dir.path().join("left.uf2"));

    let invocations = docker.invocations();
    assert_eq!(invocations.len(), 1);
    let record = &invocations[0];
    assert_eq!(record.image, "ghcr.io/moergo/custom");
    assert_eq!(
        record.env.get("MOERGO_BOARD").map(String::as_str),
        Some("nice_nano_v2")
    );
    assert_eq!(
        record.env.get("BOARD_NAME").map(String::as_str),
        Some("nice_nano_v2")
    );
    assert_eq!(
        record.env.get("MOERGO_SHIELD").map(String::as_str),
        Some("glove80_left")
    );
    assert_eq!(
        record.env.get("KEYMAP").map(String::as_str),
        Some("/workspace/config/nice_nano_v2.keymap")
    );
    assert_eq!(
        record.env.get("KCONFIG").map(String::as_str),
        Some("/workspace/config/nice_nano_v2.conf")
    );
}

#[test]
fn moergo_toolchain_accepts_keymap_inputs() {
    let manifest = FirmwareManifest::from_toml_str(include_str!("fixtures/firmware_manifest.toml"))
        .expect("manifest");
    let docker = FakeDockerBackend::new();
    docker.set_on_run(|invocation| {
        let workspace = host_workspace(invocation);
        let artifacts = workspace.join("artifacts");
        fs::create_dir_all(&artifacts).expect("artifact dir");
        fs::write(artifacts.join("right.uf2"), b"demo").expect("artifact");
        // Generated JSON should exist even when only keymap/config inputs were provided.
        assert!(workspace.join("layout/layout.json").exists());
    });
    let builder = FirmwareBuilder::new(manifest, Box::new(docker.clone()));
    let output_dir = tempdir().expect("tempdir");
    let request = builder
        .builder()
        .keyboard("glove80")
        .target("right")
        .layout_files(fixture("cli_base.dts"), Some(fixture("sample_config.dtsi")))
        .output_dir(output_dir.path().to_path_buf())
        .build()
        .expect("request");
    let report = builder.build(request).expect("build");
    assert!(report.success);
    assert!(output_dir.path().join("right.uf2").exists());
}

#[test]
fn firmware_builder_runs_zmk_toolchain() {
    let manifest = FirmwareManifest::from_toml_str(include_str!("fixtures/firmware_manifest.toml"))
        .expect("manifest");
    let docker = FakeDockerBackend::new();
    docker.set_on_run(|invocation| {
        let workspace = host_workspace(invocation);
        match invocation.command.get(1).map(String::as_str) {
            Some("init") => {
                let west_dir = workspace.join("app/.west");
                fs::create_dir_all(&west_dir).expect("west dir");
                fs::write(west_dir.join("config"), b"init").expect("west config");
            }
            Some("build") => {
                let build_dir = workspace.join("build/right/zephyr");
                fs::create_dir_all(&build_dir).expect("build dir");
                fs::write(build_dir.join("firmware.uf2"), b"demo").expect("artifact");
                let config_path =
                    workspace.join("config/boards/shields/glove80_right/glove80_right.conf");
                let config_text = fs::read_to_string(&config_path).expect("config text");
                assert!(
                    config_text.contains("CONFIG_TEST_FEATURE=\"demo\""),
                    "kconfig definitions should be appended to the staged config"
                );
            }
            _ => {}
        }
    });
    let builder = FirmwareBuilder::new(manifest, Box::new(docker.clone()));
    let output_dir = tempdir().expect("tempdir");
    let request = builder
        .builder()
        .keyboard("glove80")
        .toolchain("zmk")
        .target("right")
        .kconfig_def("CONFIG_TEST_FEATURE", "\"demo\"")
        .layout_files(fixture("cli_base.dts"), Some(fixture("sample_config.dtsi")))
        .output_dir(output_dir.path().to_path_buf())
        .build()
        .expect("request");
    let report = builder.build(request).expect("build");
    assert!(report.success);
    assert!(output_dir.path().join("firmware.uf2").exists());
    let log_path = report.logs_path.as_ref().expect("log path");
    assert!(log_path.exists(), "log file should exist");
    let log_text = fs::read_to_string(log_path).expect("log text");
    assert!(
        log_text.contains("completed target right"),
        "log should record successful target completion"
    );
    let info_path = report.build_info_path.as_ref().expect("build info path");
    assert!(info_path.exists(), "build-info file should exist");
    let info_text = fs::read_to_string(info_path).expect("info text");
    let info: Value = serde_json::from_str(&info_text).expect("info json");
    assert_eq!(info["keyboard"], "glove80");
    assert_eq!(info["toolchain"], "zmk");
    assert_eq!(info["success"].as_bool(), Some(true));

    let invocations = docker.invocations();
    assert_eq!(invocations.len(), 4);
    assert_eq!(
        invocations[0].command.get(1).map(String::as_str),
        Some("init")
    );
    assert_eq!(
        invocations[1].command.get(1).map(String::as_str),
        Some("update")
    );
    assert_eq!(
        invocations[2].command.get(1).map(String::as_str),
        Some("zephyr-export")
    );
    let record = invocations.last().expect("build invocation");
    assert_eq!(record.image, "zmkfirmware/zmk-build-arm");
    assert!(
        record
            .command
            .iter()
            .any(|arg| arg == "--" || arg.starts_with("-DZMK_CONFIG="))
    );
    assert!(
        record
            .command
            .iter()
            .any(|arg| arg.contains("-DSHIELD=glove80_right")),
        "shield should be passed to west build"
    );
    assert!(
        record
            .command
            .iter()
            .any(|arg| arg == "-DCONFIG_TEST_FEATURE=\"demo\""),
        "kconfig -D definitions should propagate to west build"
    );
    assert_eq!(
        record.env.get("ZMK_CONFIG").map(String::as_str),
        Some("/workspace/config")
    );
}

#[test]
fn firmware_builder_emits_progress_updates() {
    let manifest = FirmwareManifest::from_toml_str(include_str!("fixtures/firmware_manifest.toml"))
        .expect("manifest");
    let docker = FakeDockerBackend::new();
    docker.set_on_run(|invocation| {
        let workspace = host_workspace(invocation);
        let artifacts = workspace.join("artifacts");
        if artifacts.exists() {
            fs::remove_dir_all(&artifacts).expect("clean artifacts");
        }
        fs::create_dir_all(&artifacts).expect("artifact dir");
        let artifact_name = invocation
            .env
            .get("ARTIFACT_NAME")
            .cloned()
            .unwrap_or_else(|| "firmware".into());
        fs::write(artifacts.join(format!("{artifact_name}.uf2")), b"demo").expect("artifact");
    });
    let progress = RecordingProgress::new();
    let progress_handle: Arc<dyn ProgressReporter> = Arc::new(progress.clone());
    let builder = FirmwareBuilder::new(manifest, Box::new(docker.clone()));
    let output_dir = tempdir().expect("tempdir");
    let request = builder
        .builder()
        .keyboard("glove80")
        .layout_files(fixture("cli_base.dts"), Some(fixture("sample_config.dtsi")))
        .output_dir(output_dir.path().to_path_buf())
        .progress(progress_handle)
        .build()
        .expect("request");
    let report = builder.build(request).expect("build");
    assert_eq!(
        report
            .metadata
            .entries
            .get("targets.completed")
            .map(String::as_str),
        Some("left,right")
    );
    assert_eq!(
        report
            .metadata
            .entries
            .get("artifacts.count")
            .map(String::as_str),
        Some("2")
    );
    let updates = progress.updates();
    assert_eq!(updates.len(), 4);
    assert_eq!(updates[0], (0, 2, "building target left".to_string()));
    assert_eq!(updates[1], (1, 2, "completed target left".to_string()));
    assert_eq!(updates[2], (1, 2, "building target right".to_string()));
    assert_eq!(updates[3], (2, 2, "completed target right".to_string()));
    assert_eq!(progress.starts().len(), 3);
    assert_eq!(progress.completions().len(), 3);
    assert!(progress.failures().is_empty());
}

#[test]
fn workspace_cache_hydrates_existing_files() {
    let manifest = FirmwareManifest::from_toml_str(
        r#"
version = 1

[toolchains.moergo]
kind = "moergo"
image = "ghcr.io/demo/moergo"
[toolchains.moergo.cache]
workspace = "read_only"

[keyboards.demo]
default_toolchain = "moergo"

[[keyboards.demo.targets]]
id = "main"
board = "nice_nano_v2"
"#,
    )
    .expect("manifest");
    let cache_dir = tempdir().expect("cache dir");
    let cache_root = cache_dir.path().join("cache");
    let cache_app = cache_root
        .join("firmware")
        .join("moergo")
        .join("workspace")
        .join("app");
    fs::create_dir_all(&cache_app).expect("cache app dir");
    fs::write(cache_app.join("cached.txt"), b"cached").expect("cache file");

    let docker = FakeDockerBackend::new();
    docker.set_on_run(|invocation| {
        let workspace = host_workspace(invocation);
        let cached = workspace.join("app/cached.txt");
        assert!(cached.exists(), "workspace cache should hydrate files");
    });

    let builder = FirmwareBuilder::new(manifest, Box::new(docker.clone()))
        .with_workspace_manager(WorkspaceManager::with_cache_root(cache_root));
    let output_dir = tempdir().expect("tempdir");
    let request = builder
        .builder()
        .keyboard("demo")
        .target("main")
        .layout_files(
            fixture("sample_keymap.dtsi"),
            Some(fixture("sample_config.dtsi")),
        )
        .output_dir(output_dir.path().to_path_buf())
        .build()
        .expect("request");
    let report = builder.build(request).expect("build");
    assert!(report.success);
}

#[test]
fn workspace_cache_persists_when_read_write() {
    let manifest = FirmwareManifest::from_toml_str(
        r#"
version = 1

[toolchains.zmk]
kind = "zmk_config"
image = "zmkfirmware/zmk-build-arm"
[toolchains.zmk.cache]
workspace = "read_write"
build = "read_write"

[keyboards.demo]
default_toolchain = "zmk"

[[keyboards.demo.targets]]
id = "main"
board = "nice_nano_v2"
shield = "demo"
"#,
    )
    .expect("manifest");
    let cache_dir = tempdir().expect("cache dir");
    let cache_root = cache_dir.path().join("cache");
    let docker = FakeDockerBackend::new();
    docker.set_on_run(|invocation| {
        let workspace = host_workspace(invocation);
        match invocation.command.get(1).map(String::as_str) {
            Some("init") => {
                let west_dir = workspace.join("app/.west");
                fs::create_dir_all(&west_dir).expect("west dir");
                fs::write(west_dir.join("config"), b"init").expect("west config");
                fs::write(workspace.join("app/repo.txt"), b"repo").expect("repo file");
            }
            Some("build") => {
                let build_dir = workspace.join("build/main/zephyr");
                fs::create_dir_all(&build_dir).expect("build dir");
                fs::write(build_dir.join("firmware.uf2"), b"demo").expect("artifact");
            }
            _ => {}
        }
    });

    let builder = FirmwareBuilder::new(manifest, Box::new(docker.clone()))
        .with_workspace_manager(WorkspaceManager::with_cache_root(cache_root.clone()));
    let output_dir = tempdir().expect("output dir");
    let request = builder
        .builder()
        .keyboard("demo")
        .toolchain("zmk")
        .target("main")
        .layout_files(
            fixture("sample_keymap.dtsi"),
            Some(fixture("sample_config.dtsi")),
        )
        .output_dir(output_dir.path().to_path_buf())
        .build()
        .expect("request");
    builder.build(request).expect("build");

    let workspace_cache = cache_root
        .join("firmware")
        .join("zmk")
        .join("workspace")
        .join("app")
        .join("repo.txt");
    assert!(
        workspace_cache.exists(),
        "workspace cache should persist files"
    );
    let build_cache = cache_root
        .join("firmware")
        .join("zmk")
        .join("build")
        .join("main")
        .join("zephyr")
        .join("firmware.uf2");
    assert!(build_cache.exists(), "build cache should persist artifacts");
}

#[test]
fn disable_cache_flag_skips_cache_usage() {
    let manifest = FirmwareManifest::from_toml_str(
        r#"
version = 1

[toolchains.zmk]
kind = "zmk_config"
image = "zmkfirmware/zmk-build-arm"
[toolchains.zmk.cache]
workspace = "read_write"
build = "read_write"

[keyboards.demo]
default_toolchain = "zmk"

[[keyboards.demo.targets]]
id = "main"
board = "nice_nano_v2"
"#,
    )
    .expect("manifest");
    let cache_dir = tempdir().expect("cache dir");
    let cache_root = cache_dir.path().join("cache");
    let workspace_cache = cache_root
        .join("firmware")
        .join("zmk")
        .join("workspace")
        .join("app");
    fs::create_dir_all(&workspace_cache).expect("cache app dir");
    fs::write(workspace_cache.join("cached.txt"), b"cached").expect("cache seed");

    let docker = FakeDockerBackend::new();
    docker.set_on_run(|invocation| {
        let workspace = host_workspace(invocation);
        assert!(
            !workspace.join("app/cached.txt").exists(),
            "disable_cache should prevent hydration"
        );
        match invocation.command.get(1).map(String::as_str) {
            Some("init") => {
                let west_dir = workspace.join("app/.west");
                fs::create_dir_all(&west_dir).expect("west dir");
                fs::write(west_dir.join("config"), b"init").expect("west config");
            }
            Some("build") => {
                let build_dir = workspace.join("build/main/zephyr");
                fs::create_dir_all(&build_dir).expect("build dir");
                fs::write(workspace.join("app/runtime.txt"), b"runtime").expect("runtime file");
                fs::write(build_dir.join("firmware.uf2"), b"demo").expect("artifact");
            }
            _ => {}
        }
    });

    let builder = FirmwareBuilder::new(manifest, Box::new(docker.clone()))
        .with_workspace_manager(WorkspaceManager::with_cache_root(cache_root.clone()));
    let output_dir = tempdir().expect("output dir");
    let request = builder
        .builder()
        .keyboard("demo")
        .toolchain("zmk")
        .target("main")
        .layout_files(
            fixture("sample_keymap.dtsi"),
            Some(fixture("sample_config.dtsi")),
        )
        .output_dir(output_dir.path().to_path_buf())
        .disable_cache(true)
        .build()
        .expect("request");
    builder.build(request).expect("build");

    assert!(
        !workspace_cache.join("runtime.txt").exists(),
        "runtime files should not be written back when cache disabled"
    );
    let build_cache = cache_root.join("firmware").join("zmk").join("build");
    assert!(
        !build_cache.exists(),
        "build cache should remain untouched when cache disabled"
    );
}

fn host_workspace(invocation: &DockerInvocation) -> PathBuf {
    invocation
        .volumes
        .iter()
        .find(|mount| mount.container_path == PathBuf::from("/workspace"))
        .expect("workspace mount")
        .host_path
        .clone()
}

#[derive(Clone, Default)]
struct RecordingProgress {
    updates: Arc<Mutex<Vec<(u32, u32, String)>>>,
    starts: Arc<Mutex<Vec<String>>>,
    completions: Arc<Mutex<Vec<String>>>,
    failures: Arc<Mutex<Vec<String>>>,
}

impl RecordingProgress {
    fn new() -> Self {
        Self::default()
    }

    fn updates(&self) -> Vec<(u32, u32, String)> {
        self.updates.lock().expect("lock updates").clone()
    }

    fn starts(&self) -> Vec<String> {
        self.starts.lock().expect("lock starts").clone()
    }

    fn completions(&self) -> Vec<String> {
        self.completions.lock().expect("lock completes").clone()
    }

    fn failures(&self) -> Vec<String> {
        self.failures.lock().expect("lock failures").clone()
    }
}

impl ProgressReporter for RecordingProgress {
    fn log(&self, _level: LogLevel, _message: &str) {}

    fn start_checkpoint(&self, id: &str, _message: &str) {
        self.starts
            .lock()
            .expect("lock starts")
            .push(id.to_string());
    }

    fn complete_checkpoint(&self, id: &str) {
        self.completions
            .lock()
            .expect("lock completes")
            .push(id.to_string());
    }

    fn fail_checkpoint(&self, id: &str) {
        self.failures
            .lock()
            .expect("lock failures")
            .push(id.to_string());
    }

    fn update_progress(&self, current: u32, total: u32, status: &str) {
        self.updates
            .lock()
            .expect("lock updates")
            .push((current, total, status.to_string()));
    }
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
