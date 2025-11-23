use regex::Regex;
use serde::Deserialize;
#[cfg(feature = "flash-fake-backend")]
use std::env;
#[cfg(feature = "flash-fake-backend")]
use std::sync::{Arc, OnceLock, RwLock};
use std::{
    collections::HashSet,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    time::Duration,
};
use thiserror::Error;

use crate::profiles::{HardwareFlash, KeyboardProfileDoc};

use super::platform;

#[cfg(feature = "flash-fake-backend")]
static TEST_BACKEND: RwLock<Option<Arc<dyn FlashBackend + Send + Sync>>> = RwLock::new(None);
#[cfg(feature = "flash-fake-backend")]
static ENV_BACKEND_INSTALL: OnceLock<()> = OnceLock::new();

pub(crate) trait FlashBackend {
    fn discover_devices(&self, config: &FlashConfig) -> Result<Vec<FlashDiscovery>, FlashError>;
    fn wait_for_device(
        &self,
        config: &FlashConfig,
        target: Option<&FlashTarget>,
        seen_serials: &HashSet<String>,
    ) -> Result<FlashDevice, FlashError>;
}

#[derive(Clone, Copy, Default)]
struct PlatformBackend;

impl FlashBackend for PlatformBackend {
    fn discover_devices(&self, config: &FlashConfig) -> Result<Vec<FlashDiscovery>, FlashError> {
        platform::discover_devices(config)
    }

    fn wait_for_device(
        &self,
        config: &FlashConfig,
        target: Option<&FlashTarget>,
        seen_serials: &HashSet<String>,
    ) -> Result<FlashDevice, FlashError> {
        platform::wait_for_device(config, target, seen_serials)
    }
}

#[cfg(feature = "flash-fake-backend")]
#[derive(Clone)]
struct EnvFlashBackend {
    discovery: FlashDiscovery,
    device: FlashDevice,
}

#[cfg(feature = "flash-fake-backend")]
impl EnvFlashBackend {
    fn from_env() -> Option<Self> {
        if env::var("ZMK_FLASH_FAKE_BACKEND").is_err() {
            return None;
        }
        let mount = env::var("ZMK_FLASH_FAKE_MOUNTPOINT").ok()?;
        let mountpoint = PathBuf::from(mount);
        let name = env::var("ZMK_FLASH_FAKE_NAME").unwrap_or_else(|_| "FAKE_FLASH_DEVICE".into());
        let dev_path = env::var("ZMK_FLASH_FAKE_DEVPATH").unwrap_or_else(|_| "/dev/fake".into());
        let serial = env::var("ZMK_FLASH_FAKE_SERIAL").ok();
        let vendor = env::var("ZMK_FLASH_FAKE_VENDOR").ok();
        let model = env::var("ZMK_FLASH_FAKE_MODEL").ok();
        let fs_type = env::var("ZMK_FLASH_FAKE_FSTYPE").ok();
        let removable = env::var("ZMK_FLASH_FAKE_REMOVABLE")
            .ok()
            .map(|v| v == "true");

        let discovery = FlashDiscovery {
            name: name.clone(),
            dev_path: Some(PathBuf::from(&dev_path)),
            mountpoints: vec![mountpoint.clone()],
            serial: serial.clone(),
            vendor: vendor.clone(),
            model: model.clone(),
            fs_type: fs_type.clone(),
            removable,
            vendor_id: None,
            product_id: None,
        };
        let device = FlashDevice {
            name,
            dev_path: Some(PathBuf::from(dev_path)),
            mountpoint,
            serial,
            vendor,
            model,
            fs_type,
            auto_unmount: false,
            cleanup_path: None,
            vendor_id: None,
            product_id: None,
        };
        Some(Self { discovery, device })
    }
}

#[cfg(feature = "flash-fake-backend")]
impl FlashBackend for EnvFlashBackend {
    fn discover_devices(&self, _config: &FlashConfig) -> Result<Vec<FlashDiscovery>, FlashError> {
        Ok(vec![self.discovery.clone()])
    }

    fn wait_for_device(
        &self,
        _config: &FlashConfig,
        _target: Option<&FlashTarget>,
        _seen_serials: &HashSet<String>,
    ) -> Result<FlashDevice, FlashError> {
        Ok(self.device.clone())
    }
}

/// Logical half to flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashSide {
    Left,
    Right,
}

impl FlashSide {
    pub fn as_str(&self) -> &'static str {
        match self {
            FlashSide::Left => "left",
            FlashSide::Right => "right",
        }
    }
}

impl std::fmt::Display for FlashSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Artifact selection for flashing.
#[derive(Debug, Clone)]
pub enum FlashSource {
    Single(PathBuf),
    Split { left: PathBuf, right: PathBuf },
}

impl FlashSource {
    /// Fetch the artifact path to use for the given side.
    pub fn artifact_for_side(&self, side: FlashSide) -> Option<&Path> {
        match self {
            FlashSource::Single(path) => Some(path.as_path()),
            FlashSource::Split { left, right } => match side {
                FlashSide::Left => Some(left.as_path()),
                FlashSide::Right => Some(right.as_path()),
            },
        }
    }
}

/// Flash configuration derived from hardware profile or CLI overrides.
#[derive(Debug, Clone)]
pub struct FlashConfig {
    pub device_query: Option<String>,
    pub mount_timeout: Duration,
    pub copy_timeout: Duration,
    pub sync_after_copy: bool,
}

impl Default for FlashConfig {
    fn default() -> Self {
        Self {
            device_query: None,
            mount_timeout: Duration::from_secs(60),
            copy_timeout: Duration::from_secs(60),
            sync_after_copy: false,
        }
    }
}

impl From<&HardwareFlash> for FlashConfig {
    fn from(flash: &HardwareFlash) -> Self {
        let mut config = FlashConfig::default();
        if let Some(query) = flash.device_query.as_deref() {
            config.device_query = Some(query.trim().to_string());
        }
        if let Some(seconds) = flash.mount_timeout {
            config.mount_timeout = Duration::from_secs(seconds.into());
        }
        if let Some(seconds) = flash.copy_timeout {
            config.copy_timeout = Duration::from_secs(seconds.into());
        }
        if let Some(sync) = flash.sync_after_copy {
            config.sync_after_copy = sync;
        }
        config
    }
}

pub(crate) fn flash_debug(message: impl AsRef<str>) {
    log::debug!("{}", message.as_ref());
}

/// Target with side + board metadata.
#[derive(Debug, Clone)]
pub struct FlashTarget {
    pub side: FlashSide,
    pub board_id: Option<String>,
    pub config: FlashConfig,
}

/// Discovered device ready for flashing.
#[derive(Debug, Clone)]
pub struct FlashDevice {
    pub name: String,
    pub dev_path: Option<PathBuf>,
    pub mountpoint: PathBuf,
    pub serial: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub fs_type: Option<String>,
    pub auto_unmount: bool,
    pub cleanup_path: Option<PathBuf>,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
}

/// Probe result describing a connected storage device.
#[derive(Debug, Clone)]
pub struct FlashDiscovery {
    pub name: String,
    pub dev_path: Option<PathBuf>,
    pub mountpoints: Vec<PathBuf>,
    pub serial: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub fs_type: Option<String>,
    pub removable: Option<bool>,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
}

/// Result of a single flash attempt.
#[derive(Debug, Clone)]
pub struct FlashOutcome {
    pub side: FlashSide,
    pub artifact: PathBuf,
    pub mountpoint: PathBuf,
    pub bytes_written: u64,
    pub warnings: Vec<String>,
}

/// Render a discovery record into the CLI-friendly summary line.
pub fn render_device(discovery: &FlashDiscovery) -> String {
    let mountpoints = if discovery.mountpoints.is_empty() {
        "<not mounted>".to_string()
    } else {
        discovery
            .mountpoints
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let dev_path = discovery
        .dev_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "-".into());
    let fs_type = discovery.fs_type.as_deref().unwrap_or("-");
    let serial = discovery.serial.as_deref().unwrap_or("-");
    let vendor = discovery.vendor.as_deref().unwrap_or("-");
    let model = discovery.model.as_deref().unwrap_or("-");
    let removable = discovery
        .removable
        .map(|r| if r { "removable" } else { "fixed" })
        .unwrap_or("-");
    format!(
        "{}  dev={}  mount={}  fs={}  serial={}  vendor={}  model={}  {}",
        discovery.name, dev_path, mountpoints, fs_type, serial, vendor, model, removable
    )
}

/// Render the flash summary line printed after flashing a side.
pub fn render_flash_outcome(outcome: &FlashOutcome) -> String {
    format!(
        "flashed {} using {} ({} bytes)",
        outcome.side,
        outcome.mountpoint.display(),
        outcome.bytes_written
    )
}

/// Render a warning message emitted during flashing.
pub fn render_flash_warning(warning: &str) -> String {
    format!("note: {warning}")
}

#[derive(Debug, Error)]
pub enum FlashError {
    #[error("no artifact available for side {0}")]
    MissingArtifact(FlashSide),
    #[error("no device matching query `{0}` was found or mounted in time")]
    NoMatchingDevice(String),
    #[error("found device `{name}` but it was not mounted")]
    UnmountedDevice { name: String },
    #[error("automatic device discovery is not available on this platform: {0}")]
    UnsupportedPlatform(String),
    #[error("failed to probe devices: {0}")]
    ProbeFailed(String),
    #[error("invalid device query `{0}`")]
    InvalidQuery(String),
    #[error("failed to read build-info {path}: {source}")]
    BuildInfoRead { path: PathBuf, source: io::Error },
    #[error("failed to parse build-info {path}: {source}")]
    BuildInfoParse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("no UF2 artifacts found in {0}")]
    NoArtifactsFound(PathBuf),
    #[error("failed to copy firmware to {dest}: {source}")]
    Copy { dest: PathBuf, source: io::Error },
    #[error("device serial {serial} was already flashed in this run")]
    DuplicateSerial { serial: String },
    #[error("board-id mismatch for {device}: expected {expected}, found {found}")]
    BoardIdMismatch {
        device: String,
        expected: String,
        found: String,
    },
    #[error("udisksctl is required for automatic mounting on Linux")]
    MissingUdisksctl,
    #[error("udisksctl mount failed for {device}: {message}")]
    UdisksctlMount { device: String, message: String },
    #[error("unable to parse udisksctl output for {device}")]
    UdisksctlOutput { device: String },
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

/// List the currently connected storage devices that match the flash query.
pub fn discover_devices(config: &FlashConfig) -> Result<Vec<FlashDiscovery>, FlashError> {
    #[cfg(feature = "flash-fake-backend")]
    {
        ensure_env_backend();
        if let Some(backend) = test_backend() {
            return discover_devices_with_backend(backend.as_ref(), config);
        }
    }
    discover_devices_with_backend(&PlatformBackend, config)
}

pub(crate) fn discover_devices_with_backend(
    backend: &dyn FlashBackend,
    config: &FlashConfig,
) -> Result<Vec<FlashDiscovery>, FlashError> {
    backend.discover_devices(config)
}

/// Create flash targets from a keyboard profile and side selection.
pub fn build_flash_targets(profile: &KeyboardProfileDoc, sides: &[FlashSide]) -> Vec<FlashTarget> {
    let flash_config = profile
        .hardware
        .flash
        .first()
        .map(FlashConfig::from)
        .unwrap_or_default();
    sides
        .iter()
        .map(|side| FlashTarget {
            side: *side,
            board_id: board_id_for_side(profile, *side),
            config: flash_config.clone(),
        })
        .collect()
}

fn board_id_for_side(profile: &KeyboardProfileDoc, side: FlashSide) -> Option<String> {
    let role = side.as_str();
    profile
        .hardware
        .boards
        .iter()
        .find(|board| {
            board
                .role
                .as_deref()
                .map_or(false, |r| r.eq_ignore_ascii_case(role))
        })
        .map(|board| board.id.clone())
}

/// Resolve a flash source from CLI-like inputs.
#[allow(clippy::too_many_arguments)]
pub fn resolve_flash_source(
    single: Option<PathBuf>,
    left: Option<PathBuf>,
    right: Option<PathBuf>,
    build_info: Option<&Path>,
    artifacts_dir: Option<&Path>,
    required_sides: &[FlashSide],
) -> Result<FlashSource, FlashError> {
    if left.is_some() || right.is_some() {
        let left_path = left.or_else(|| single.clone());
        let right_path = right.or_else(|| single.clone());
        let missing_left = required_sides.contains(&FlashSide::Left) && left_path.is_none();
        let missing_right = required_sides.contains(&FlashSide::Right) && right_path.is_none();
        if missing_left {
            return Err(FlashError::MissingArtifact(FlashSide::Left));
        }
        if missing_right {
            return Err(FlashError::MissingArtifact(FlashSide::Right));
        }
        if required_sides.len() == 1 {
            let side = required_sides[0];
            if let Some(path) = match side {
                FlashSide::Left => left_path.as_ref().or(right_path.as_ref()),
                FlashSide::Right => right_path.as_ref().or(left_path.as_ref()),
            } {
                return Ok(FlashSource::Single(path.clone()));
            }
        }
        let left_final = left_path.clone().or_else(|| right_path.clone()).unwrap();
        let right_final = right_path.clone().or_else(|| left_path.clone()).unwrap();
        return Ok(FlashSource::Split {
            left: left_final,
            right: right_final,
        });
    }

    if let Some(path) = single {
        return Ok(FlashSource::Single(path));
    }

    if let Some(path) = build_info {
        if let Some(source) = flash_source_from_build_info(path, required_sides)? {
            return Ok(source);
        }
    }

    if let Some(dir) = artifacts_dir {
        return flash_source_from_directory(dir, required_sides);
    }

    Err(FlashError::InvalidArgument(
        "no firmware artifact specified (use --firmware, --left/--right, --build-info, or --artifacts)"
            .into(),
    ))
}

/// Flash a single target. If `mount_override` is None, automatic discovery is attempted.
pub fn flash_target(
    target: &FlashTarget,
    source: &FlashSource,
    mount_override: Option<&Path>,
    seen_serials: &mut HashSet<String>,
) -> Result<FlashOutcome, FlashError> {
    #[cfg(feature = "flash-fake-backend")]
    {
        ensure_env_backend();
        if let Some(backend) = test_backend() {
            return flash_target_with_backend(
                backend.as_ref(),
                target,
                source,
                mount_override,
                seen_serials,
            );
        }
    }
    flash_target_with_backend(
        &PlatformBackend,
        target,
        source,
        mount_override,
        seen_serials,
    )
}

pub(crate) fn flash_target_with_backend(
    backend: &dyn FlashBackend,
    target: &FlashTarget,
    source: &FlashSource,
    mount_override: Option<&Path>,
    seen_serials: &mut HashSet<String>,
) -> Result<FlashOutcome, FlashError> {
    let artifact = source
        .artifact_for_side(target.side)
        .ok_or(FlashError::MissingArtifact(target.side))?;
    flash_debug(format!(
        "flash_target side={} artifact={} mount_override={}",
        target.side,
        artifact.display(),
        mount_override
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<auto>".into())
    ));
    let device = if let Some(path) = mount_override {
        FlashDevice {
            name: path.display().to_string(),
            dev_path: None,
            mountpoint: path.to_path_buf(),
            serial: None,
            vendor: None,
            model: None,
            fs_type: None,
            auto_unmount: false,
            cleanup_path: None,
            vendor_id: None,
            product_id: None,
        }
    } else {
        wait_for_device(backend, &target.config, Some(target), seen_serials)?
    };
    flash_debug(format!(
        "using device name={} dev_path={} mountpoint={} serial={}",
        device.name,
        device
            .dev_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".into()),
        device.mountpoint.display(),
        device.serial.as_deref().unwrap_or("-")
    ));
    if let Some(serial) = device.serial.as_ref() {
        if seen_serials.contains(serial) {
            return Err(FlashError::DuplicateSerial {
                serial: serial.clone(),
            });
        }
    }
    let mut warnings = Vec::new();
    if let Some(expected) = target.board_id.as_ref() {
        match read_board_id(&device.mountpoint) {
            Some(found) => {
                if !found.eq_ignore_ascii_case(expected) && !found.contains(expected) {
                    return Err(FlashError::BoardIdMismatch {
                        device: device.name.clone(),
                        expected: expected.clone(),
                        found,
                    });
                }
            }
            None => warnings.push(format!(
                "could not read board-id from {} to verify side",
                device.mountpoint.display()
            )),
        }
    }
    let (bytes_written, mut copy_warnings) =
        copy_to_mountpoint(artifact, &device.mountpoint, target.config.sync_after_copy)?;
    flash_debug(format!(
        "copy finished side={} bytes={} mountpoint={}",
        target.side,
        bytes_written,
        device.mountpoint.display()
    ));
    warnings.append(&mut copy_warnings);
    if let Some(serial) = device.serial.clone() {
        seen_serials.insert(serial.clone());
        warnings.push(format!("flashed device serial {}", serial));
    }
    if device.auto_unmount {
        #[cfg(target_os = "linux")]
        {
            if let Some(dev_path) = device.dev_path.clone() {
                match std::process::Command::new("udisksctl")
                    .args(["unmount", "-b", dev_path.to_str().unwrap_or_default()])
                    .output()
                {
                    Ok(output) if output.status.success() => {
                        if let Some(path) = device.cleanup_path {
                            let _ = fs::remove_dir_all(path);
                        }
                    }
                    Ok(output) => warnings.push(format!(
                        "failed to unmount {}: {}",
                        device.mountpoint.display(),
                        String::from_utf8_lossy(&output.stderr).trim()
                    )),
                    Err(err) => warnings.push(format!(
                        "failed to unmount {}: {}",
                        device.mountpoint.display(),
                        err
                    )),
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(dev_path) = device.dev_path.clone() {
                match std::process::Command::new("diskutil")
                    .args(["unmountDisk", dev_path.to_str().unwrap_or_default()])
                    .output()
                {
                    Ok(output) if output.status.success() => {}
                    Ok(output) => warnings.push(format!(
                        "failed to unmount {}: {}",
                        device.mountpoint.display(),
                        String::from_utf8_lossy(&output.stderr).trim()
                    )),
                    Err(err) => warnings.push(format!(
                        "failed to unmount {}: {}",
                        device.mountpoint.display(),
                        err
                    )),
                }
            }
        }
    }
    Ok(FlashOutcome {
        side: target.side,
        artifact: artifact.to_path_buf(),
        mountpoint: device.mountpoint,
        bytes_written,
        warnings,
    })
}

/// Determine default sides based on side flag and hardware split-ness.
pub fn default_sides(
    profile: Option<&KeyboardProfileDoc>,
    side_flag: Option<FlashSideSelection>,
) -> Vec<FlashSide> {
    if let Some(flag) = side_flag {
        return match flag {
            FlashSideSelection::Left => vec![FlashSide::Left],
            FlashSideSelection::Right => vec![FlashSide::Right],
            FlashSideSelection::Both => vec![FlashSide::Left, FlashSide::Right],
        };
    }
    if profile.map(|doc| doc.hardware.is_split).unwrap_or(false) {
        vec![FlashSide::Left, FlashSide::Right]
    } else {
        vec![FlashSide::Left]
    }
}

/// Side selector exposed to the CLI layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashSideSelection {
    Left,
    Right,
    Both,
}

#[cfg(feature = "flash-fake-backend")]
pub(crate) fn set_test_flash_backend<B: FlashBackend + Send + Sync + 'static>(backend: B) {
    let mut guard = TEST_BACKEND.write().expect("test backend lock poisoned");
    *guard = Some(Arc::new(backend));
}

#[cfg(feature = "flash-fake-backend")]
pub fn clear_test_flash_backend() {
    let mut guard = TEST_BACKEND.write().expect("test backend lock poisoned");
    *guard = None;
}

fn flash_source_from_build_info(
    build_info: &Path,
    required_sides: &[FlashSide],
) -> Result<Option<FlashSource>, FlashError> {
    let info = load_build_info(build_info)?;
    let base_dir = build_info
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let left = artifact_for_side_from_build_info(&info, FlashSide::Left)
        .map(|p| resolve_relative(&base_dir, p));
    let right = artifact_for_side_from_build_info(&info, FlashSide::Right)
        .map(|p| resolve_relative(&base_dir, p));

    if required_sides.len() == 1 {
        if let Some(path) = match required_sides[0] {
            FlashSide::Left => left.clone().or(right.clone()),
            FlashSide::Right => right.clone().or(left.clone()),
        } {
            return Ok(Some(FlashSource::Single(path)));
        }
    }
    if let (Some(left_path), Some(right_path)) = (left, right) {
        return Ok(Some(FlashSource::Split {
            left: left_path,
            right: right_path,
        }));
    }
    if required_sides.len() > 1 && !info.artifacts.is_empty() {
        let path = resolve_relative(&base_dir, info.artifacts[0].clone());
        return Ok(Some(FlashSource::Single(path)));
    }
    Ok(None)
}

fn artifact_for_side_from_build_info(info: &BuildInfo, side: FlashSide) -> Option<PathBuf> {
    let id = side.as_str();
    info.targets
        .iter()
        .find(|target| target.id.eq_ignore_ascii_case(id))
        .and_then(|target| target.artifacts.first())
        .cloned()
}

fn resolve_relative(base: &Path, value: PathBuf) -> PathBuf {
    if value.is_absolute() {
        value
    } else {
        base.join(value)
    }
}

fn flash_source_from_directory(
    dir: &Path,
    required_sides: &[FlashSide],
) -> Result<FlashSource, FlashError> {
    let mut uf2_files = Vec::new();
    for entry in fs::read_dir(dir).map_err(|_| FlashError::NoArtifactsFound(dir.to_path_buf()))? {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map_or(false, |ext| ext.eq_ignore_ascii_case("uf2"))
        {
            uf2_files.push(path);
        }
    }
    if uf2_files.is_empty() {
        return Err(FlashError::NoArtifactsFound(dir.to_path_buf()));
    }
    if required_sides.len() == 1 || uf2_files.len() == 1 {
        return Ok(FlashSource::Single(uf2_files[0].clone()));
    }
    let mut left = None;
    let mut right = None;
    for path in &uf2_files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if left.is_none() && name.contains("left") {
            left = Some(path.clone());
        }
        if right.is_none() && name.contains("right") {
            right = Some(path.clone());
        }
    }
    if let (Some(l), Some(r)) = (left, right) {
        return Ok(FlashSource::Split { left: l, right: r });
    }
    Ok(FlashSource::Single(uf2_files[0].clone()))
}

pub(crate) fn copy_to_mountpoint(
    artifact: &Path,
    mountpoint: &Path,
    sync_after_copy: bool,
) -> Result<(u64, Vec<String>), FlashError> {
    if !mountpoint.is_dir() {
        return Err(FlashError::InvalidArgument(format!(
            "mount path {} is not a directory",
            mountpoint.display()
        )));
    }
    let filename = artifact
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "firmware.uf2".into());
    let dest = mountpoint.join(filename);
    let bytes_copied = fs::copy(artifact, &dest).map_err(|source| FlashError::Copy {
        dest: dest.clone(),
        source,
    })?;
    flash_debug(format!(
        "copied artifact {} to {} ({} bytes)",
        artifact.display(),
        dest.display(),
        bytes_copied
    ));
    let mut warnings = Vec::new();
    if sync_after_copy {
        if let Err(err) = File::open(&dest).and_then(|file| file.sync_all()) {
            warnings.push(format!("sync failed for {}: {}", dest.display(), err));
        }
        #[cfg(unix)]
        {
            if let Err(err) = File::open(mountpoint).and_then(|file| file.sync_all()) {
                warnings.push(format!(
                    "sync failed for mount {}: {}",
                    mountpoint.display(),
                    err
                ));
            }
        }
    }
    Ok((bytes_copied, warnings))
}

fn read_board_id(mountpoint: &Path) -> Option<String> {
    for filename in ["INFO_UF2.TXT", "INFO_UF2.txt", "info_uf2.txt"] {
        let candidate = mountpoint.join(filename);
        if let Ok(text) = fs::read_to_string(&candidate) {
            for line in text.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("Board-ID:") {
                    let value = rest.trim();
                    if !value.is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn wait_for_device(
    backend: &dyn FlashBackend,
    config: &FlashConfig,
    target: Option<&FlashTarget>,
    seen_serials: &HashSet<String>,
) -> Result<FlashDevice, FlashError> {
    backend.wait_for_device(config, target, seen_serials)
}

fn load_build_info(path: &Path) -> Result<BuildInfo, FlashError> {
    let data = fs::read(path).map_err(|source| FlashError::BuildInfoRead {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&data).map_err(|source| FlashError::BuildInfoParse {
        path: path.to_path_buf(),
        source,
    })
}

/// Build-info structure emitted by firmware builds.
#[derive(Debug, Deserialize)]
struct BuildInfo {
    #[serde(default)]
    artifacts: Vec<PathBuf>,
    #[serde(default)]
    targets: Vec<BuildInfoTarget>,
}

#[derive(Debug, Deserialize)]
struct BuildInfoTarget {
    id: String,
    #[serde(default)]
    artifacts: Vec<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct Query {
    pub clauses: Vec<QueryClause>,
}

impl Query {
    pub(crate) fn parse(input: &str) -> Result<Self, FlashError> {
        let mut clauses = Vec::new();
        for part in input.split("and") {
            let clause = QueryClause::parse(part.trim())?;
            clauses.push(clause);
        }
        Ok(Query { clauses })
    }

    pub(crate) fn matches(&self, meta: &QueryMetadata) -> bool {
        self.clauses.iter().all(|c| c.matches(meta))
    }
}

#[derive(Debug)]
pub(crate) enum QueryClause {
    Equals { field: QueryField, value: String },
    Regex { field: QueryField, regex: Regex },
}

impl QueryClause {
    fn parse(raw: &str) -> Result<Self, FlashError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(FlashError::InvalidQuery(raw.into()));
        }
        if let Some((field, value)) = raw.split_once("~=") {
            let field = QueryField::parse(field.trim())?;
            let regex =
                Regex::new(value.trim()).map_err(|_| FlashError::InvalidQuery(raw.into()))?;
            return Ok(QueryClause::Regex { field, regex });
        }
        if let Some((field, value)) = raw.split_once('=') {
            let field = QueryField::parse(field.trim())?;
            return Ok(QueryClause::Equals {
                field,
                value: value.trim().to_string(),
            });
        }
        Err(FlashError::InvalidQuery(raw.into()))
    }

    fn matches(&self, meta: &QueryMetadata) -> bool {
        match self {
            QueryClause::Equals { field, value } => field.equals(meta, value),
            QueryClause::Regex { field, regex } => field.regex(meta, regex),
        }
    }
}

#[derive(Debug)]
pub(crate) enum QueryField {
    Serial,
    Vendor,
    VendorId,
    Model,
    ProductId,
    FsType,
    Removable,
}

impl QueryField {
    fn parse(raw: &str) -> Result<Self, FlashError> {
        match raw.to_ascii_lowercase().as_str() {
            "serial" => Ok(QueryField::Serial),
            "vendor" => Ok(QueryField::Vendor),
            "vendor_id" | "vid" => Ok(QueryField::VendorId),
            "model" => Ok(QueryField::Model),
            "product_id" | "pid" => Ok(QueryField::ProductId),
            "fstype" | "fs_type" => Ok(QueryField::FsType),
            "removable" | "rm" => Ok(QueryField::Removable),
            _ => Err(FlashError::InvalidQuery(raw.into())),
        }
    }

    fn equals(&self, meta: &QueryMetadata, value: &str) -> bool {
        match self {
            QueryField::Serial => meta
                .serial
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::Vendor => meta
                .vendor
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::VendorId => meta
                .vendor_id
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::Model => meta
                .model
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::ProductId => meta
                .product_id
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::FsType => meta
                .fs_type
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::Removable => meta.removable.unwrap_or(false) == (value == "true"),
        }
    }

    fn regex(&self, meta: &QueryMetadata, regex: &Regex) -> bool {
        match self {
            QueryField::Serial => meta.serial.as_deref().map_or(false, |v| regex.is_match(v)),
            QueryField::Vendor => meta.vendor.as_deref().map_or(false, |v| regex.is_match(v)),
            QueryField::VendorId => meta
                .vendor_id
                .as_deref()
                .map_or(false, |v| regex.is_match(v)),
            QueryField::Model => meta.model.as_deref().map_or(false, |v| regex.is_match(v)),
            QueryField::ProductId => meta
                .product_id
                .as_deref()
                .map_or(false, |v| regex.is_match(v)),
            QueryField::FsType => meta.fs_type.as_deref().map_or(false, |v| regex.is_match(v)),
            QueryField::Removable => regex.is_match(if meta.removable.unwrap_or(false) {
                "true"
            } else {
                "false"
            }),
        }
    }
}

#[derive(Debug)]
pub(crate) struct QueryMetadata {
    pub serial: Option<String>,
    pub vendor: Option<String>,
    pub vendor_id: Option<String>,
    pub model: Option<String>,
    pub product_id: Option<String>,
    pub fs_type: Option<String>,
    pub removable: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::core::{discover_devices_with_backend, flash_target_with_backend};
    use std::{fs, path::Path};
    use tempfile::tempdir;

    struct FakeBackend {
        device: FlashDevice,
        discoveries: Vec<FlashDiscovery>,
    }

    impl FakeBackend {
        fn new(device: FlashDevice, discoveries: Vec<FlashDiscovery>) -> Self {
            Self {
                device,
                discoveries,
            }
        }
    }

    impl FlashBackend for FakeBackend {
        fn discover_devices(
            &self,
            _config: &FlashConfig,
        ) -> Result<Vec<FlashDiscovery>, FlashError> {
            Ok(self.discoveries.clone())
        }

        fn wait_for_device(
            &self,
            _config: &FlashConfig,
            _target: Option<&FlashTarget>,
            _seen_serials: &HashSet<String>,
        ) -> Result<FlashDevice, FlashError> {
            Ok(self.device.clone())
        }
    }

    #[test]
    fn duplicate_serial_detection() {
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert("ABC".into());
        let target_seen = seen.clone();
        let err = FlashError::DuplicateSerial {
            serial: "ABC".into(),
        };
        if let FlashError::DuplicateSerial { serial } = err {
            assert_eq!(serial, "ABC");
        } else {
            panic!("unexpected error variant");
        }
        assert!(target_seen.contains("ABC"));
    }

    #[test]
    fn flash_target_uses_backend_and_copies_artifact() {
        let temp = tempdir().expect("tempdir");
        let mount = temp.path().join("mnt");
        fs::create_dir_all(&mount).expect("mount dir");
        fs::write(mount.join("INFO_UF2.TXT"), "Board-ID: GLV80").expect("board id file");
        let artifact = temp.path().join("firmware.uf2");
        fs::write(&artifact, b"demo-bytes").expect("artifact");

        let device = FlashDevice {
            name: "fake".into(),
            dev_path: None,
            mountpoint: mount.clone(),
            serial: Some("SER123".into()),
            vendor: Some("Vendor".into()),
            model: Some("Model".into()),
            fs_type: Some("vfat".into()),
            auto_unmount: false,
            cleanup_path: None,
            vendor_id: None,
            product_id: None,
        };
        let backend = FakeBackend::new(device, Vec::new());
        let target = FlashTarget {
            side: FlashSide::Left,
            board_id: Some("GLV80".into()),
            config: FlashConfig::default(),
        };
        let source = FlashSource::Single(artifact.clone());
        let mut seen = HashSet::new();

        let outcome =
            flash_target_with_backend(&backend, &target, &source, None, &mut seen).expect("flash");
        assert_eq!(outcome.bytes_written, b"demo-bytes".len() as u64);
        assert!(mount.join("firmware.uf2").exists());
        assert!(seen.contains("SER123"));
    }

    #[test]
    fn discover_devices_with_fake_backend() {
        let discovery = FlashDiscovery {
            name: "usb".into(),
            dev_path: Some(PathBuf::from("/dev/sda1")),
            mountpoints: vec![PathBuf::from("/mnt/usb")],
            serial: Some("SER".into()),
            vendor: Some("Vendor".into()),
            model: Some("Model".into()),
            fs_type: Some("vfat".into()),
            removable: Some(true),
            vendor_id: None,
            product_id: None,
        };
        let backend = FakeBackend::new(
            FlashDevice {
                name: "ignored".into(),
                dev_path: None,
                mountpoint: PathBuf::from("/mnt"),
                serial: None,
                vendor: None,
                model: None,
                fs_type: None,
                auto_unmount: false,
                cleanup_path: None,
                vendor_id: None,
                product_id: None,
            },
            vec![discovery.clone()],
        );
        let devices =
            discover_devices_with_backend(&backend, &FlashConfig::default()).expect("discover");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "usb");
        assert_eq!(
            devices[0].mountpoints.first().map(PathBuf::as_path),
            Some(Path::new("/mnt/usb"))
        );
    }

    #[test]
    fn duplicate_serial_is_rejected_on_second_flash() {
        let temp = tempdir().expect("tempdir");
        let mount = temp.path().join("mnt");
        fs::create_dir_all(&mount).expect("mount dir");
        fs::write(&mount.join("INFO_UF2.TXT"), "Board-ID: GLV80").expect("board id file");
        let artifact = temp.path().join("firmware.uf2");
        fs::write(&artifact, b"demo").expect("artifact");

        let device = FlashDevice {
            name: "fake".into(),
            dev_path: None,
            mountpoint: mount.clone(),
            serial: Some("SER123".into()),
            vendor: None,
            model: None,
            fs_type: None,
            auto_unmount: false,
            cleanup_path: None,
            vendor_id: None,
            product_id: None,
        };
        let backend = FakeBackend::new(device, Vec::new());
        let target = FlashTarget {
            side: FlashSide::Left,
            board_id: Some("GLV80".into()),
            config: FlashConfig::default(),
        };
        let source = FlashSource::Single(artifact);
        let mut seen = HashSet::new();

        let _ = flash_target_with_backend(&backend, &target, &source, None, &mut seen)
            .expect("first flash succeeds");
        let err = flash_target_with_backend(&backend, &target, &source, None, &mut seen)
            .expect_err("duplicate serial should fail");
        match err {
            FlashError::DuplicateSerial { serial } => assert_eq!(serial, "SER123"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn board_id_mismatch_errors() {
        let temp = tempdir().expect("tempdir");
        let mount = temp.path().join("mnt");
        fs::create_dir_all(&mount).expect("mount dir");
        fs::write(&mount.join("INFO_UF2.TXT"), "Board-ID: OTHER").expect("board id file");
        let artifact = temp.path().join("firmware.uf2");
        fs::write(&artifact, b"demo").expect("artifact");

        let device = FlashDevice {
            name: "fake".into(),
            dev_path: None,
            mountpoint: mount.clone(),
            serial: None,
            vendor: None,
            model: None,
            fs_type: None,
            auto_unmount: false,
            cleanup_path: None,
            vendor_id: None,
            product_id: None,
        };
        let backend = FakeBackend::new(device, Vec::new());
        let target = FlashTarget {
            side: FlashSide::Left,
            board_id: Some("GLV80".into()),
            config: FlashConfig::default(),
        };
        let source = FlashSource::Single(artifact);
        let err = flash_target_with_backend(&backend, &target, &source, None, &mut HashSet::new())
            .expect_err("board mismatch");
        match err {
            FlashError::BoardIdMismatch {
                expected, found, ..
            } => {
                assert_eq!(expected, "GLV80");
                assert_eq!(found, "OTHER");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn missing_board_id_emits_warning_but_flashes() {
        let temp = tempdir().expect("tempdir");
        let mount = temp.path().join("mnt");
        fs::create_dir_all(&mount).expect("mount dir");
        let artifact = temp.path().join("firmware.uf2");
        fs::write(&artifact, b"demo").expect("artifact");

        let device = FlashDevice {
            name: "fake".into(),
            dev_path: None,
            mountpoint: mount.clone(),
            serial: None,
            vendor: None,
            model: None,
            fs_type: None,
            auto_unmount: false,
            cleanup_path: None,
            vendor_id: None,
            product_id: None,
        };
        let backend = FakeBackend::new(device, Vec::new());
        let target = FlashTarget {
            side: FlashSide::Left,
            board_id: Some("GLV80".into()),
            config: FlashConfig::default(),
        };
        let source = FlashSource::Single(artifact);
        let mut seen = HashSet::new();
        let outcome =
            flash_target_with_backend(&backend, &target, &source, None, &mut seen).expect("flash");
        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| w.contains("could not read board-id")),
            "warnings: {:?}",
            outcome.warnings
        );
    }

    #[test]
    fn mount_override_must_be_directory() {
        let temp = tempdir().expect("tempdir");
        let mount = temp.path().join("missing");
        let artifact = temp.path().join("firmware.uf2");
        fs::write(&artifact, b"demo").expect("artifact");

        let backend = FakeBackend::new(
            FlashDevice {
                name: "ignored".into(),
                dev_path: None,
                mountpoint: PathBuf::from("/tmp/ignored"),
                serial: None,
                vendor: None,
                model: None,
                fs_type: None,
                auto_unmount: false,
                cleanup_path: None,
                vendor_id: None,
                product_id: None,
            },
            Vec::new(),
        );
        let target = FlashTarget {
            side: FlashSide::Left,
            board_id: None,
            config: FlashConfig::default(),
        };
        let source = FlashSource::Single(artifact);
        let err = flash_target_with_backend(
            &backend,
            &target,
            &source,
            Some(mount.as_path()),
            &mut HashSet::new(),
        )
        .expect_err("should fail on invalid mount");
        assert!(matches!(err, FlashError::InvalidArgument(_)));
    }
}

#[cfg(feature = "flash-fake-backend")]
fn ensure_env_backend() {
    ENV_BACKEND_INSTALL.get_or_init(|| {
        if test_backend().is_some() {
            return;
        }
        if let Some(backend) = EnvFlashBackend::from_env() {
            set_test_flash_backend(backend);
        }
    });
}

#[cfg(feature = "flash-fake-backend")]
fn test_backend() -> Option<Arc<dyn FlashBackend + Send + Sync>> {
    TEST_BACKEND
        .read()
        .expect("test backend lock poisoned")
        .clone()
}
