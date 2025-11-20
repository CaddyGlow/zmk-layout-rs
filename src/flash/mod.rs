//! USB mass-storage flashing helpers (UF2-style) for keyboards such as the Glove80.

use regex::Regex;
use serde::Deserialize;
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    process::Command,
    thread::sleep,
    time::{Duration, Instant},
};
use thiserror::Error;

use crate::profiles::{HardwareFlash, KeyboardProfileDoc};

/// Logical half to flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashSide {
    Left,
    Right,
}

impl FlashSide {
    fn as_str(&self) -> &'static str {
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
    BuildInfoRead {
        path: PathBuf,
        source: io::Error,
    },
    #[error("failed to parse build-info {path}: {source}")]
    BuildInfoParse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("no UF2 artifacts found in {0}")]
    NoArtifactsFound(PathBuf),
    #[error("failed to copy firmware to {dest}: {source}")]
    Copy {
        dest: PathBuf,
        source: io::Error,
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
        .find(|board| board.role.as_deref().map_or(false, |r| r.eq_ignore_ascii_case(role)))
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
) -> Result<FlashOutcome, FlashError> {
    let artifact = source
        .artifact_for_side(target.side)
        .ok_or(FlashError::MissingArtifact(target.side))?;
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
        }
    } else {
        wait_for_device(&target.config)?
    };
    let (bytes_written, mut warnings) =
        copy_to_mountpoint(artifact, &device.mountpoint, target.config.sync_after_copy)?;
    if device.auto_unmount {
        if let Some(dev_path) = device.dev_path.clone() {
            match Command::new("udisksctl")
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
    Ok(FlashOutcome {
        side: target.side,
        artifact: artifact.to_path_buf(),
        mountpoint: device.mountpoint,
        bytes_written,
        warnings: {
            if let Some(serial) = device.serial {
                warnings.push(format!("flashed device serial {}", serial));
            }
            warnings
        },
    })
}

/// Determine default sides based on side flag and hardware split-ness.
pub fn default_sides(profile: Option<&KeyboardProfileDoc>, side_flag: Option<FlashSideSelection>) -> Vec<FlashSide> {
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
    for entry in
        fs::read_dir(dir).map_err(|_| FlashError::NoArtifactsFound(dir.to_path_buf()))?
    {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()).map_or(false, |ext| ext.eq_ignore_ascii_case("uf2")) {
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

fn copy_to_mountpoint(
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

fn wait_for_device(config: &FlashConfig) -> Result<FlashDevice, FlashError> {
    #[cfg(target_os = "linux")]
    {
        wait_for_device_linux(config)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(FlashError::UnsupportedPlatform(
            "automatic device discovery currently works on Linux only".into(),
        ))
    }
}

fn load_build_info(path: &Path) -> Result<BuildInfo, FlashError> {
    let data =
        fs::read(path).map_err(|source| FlashError::BuildInfoRead { path: path.to_path_buf(), source })?;
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

#[cfg(target_os = "linux")]
fn wait_for_device_linux(config: &FlashConfig) -> Result<FlashDevice, FlashError> {
    let deadline = Instant::now() + config.mount_timeout;
    let mut last_error = None;
    while Instant::now() < deadline {
        match probe_linux(config.device_query.as_deref()) {
            Ok(mut devices) => {
                if devices.len() == 1 {
                    let dev = devices.remove(0);
                    let (mountpoint, auto_unmount) = if let Some(first) = dev.mountpoints.get(0) {
                        (first.clone(), false)
                    } else {
                        let mount = mount_with_udisksctl(&dev.dev_path)?;
                        (mount, true)
                    };
                    return Ok(FlashDevice {
                        name: dev.name,
                        dev_path: Some(dev.dev_path),
                        mountpoint,
                        serial: dev.serial,
                        vendor: dev.vendor,
                        model: dev.model,
                        fs_type: dev.fs_type,
                        auto_unmount,
                        cleanup_path: None,
                    });
                }
                if devices.len() > 1 {
                    last_error = Some(FlashError::InvalidArgument(
                        "multiple devices matched; refine hardware.flash.device_query".into(),
                    ));
                }
            }
            Err(err) => last_error = Some(err),
        }
        sleep(Duration::from_millis(500));
    }
    Err(last_error.unwrap_or_else(|| {
        FlashError::NoMatchingDevice(
            config
                .device_query
                .clone()
                .unwrap_or_else(|| "<unspecified>".into()),
        )
    }))
}

#[cfg(target_os = "linux")]
fn mount_with_udisksctl(dev_path: &Path) -> Result<PathBuf, FlashError> {
    let dev_str = dev_path.to_str().unwrap_or_default();
    let output = Command::new("udisksctl")
        .args(["mount", "-b", dev_str, "--no-user-interaction"])
        .output()
        .map_err(|_| FlashError::MissingUdisksctl)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FlashError::UdisksctlMount {
            device: dev_str.to_string(),
            message: stderr.trim().to_string(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(path) = parse_udisk_mount_path(&stdout) {
        return Ok(path);
    }
    Err(FlashError::UdisksctlOutput {
        device: dev_str.to_string(),
    })
}

#[cfg(target_os = "linux")]
fn parse_udisk_mount_path(output: &str) -> Option<PathBuf> {
    for line in output.lines() {
        if let Some(idx) = line.rfind(" at ") {
            let mount = line[idx + 4..].trim().trim_end_matches('.');
            if !mount.is_empty() {
                return Some(PathBuf::from(mount));
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LsblkDevice {
    pub name: String,
    pub dev_path: PathBuf,
    pub serial: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub fs_type: Option<String>,
    pub removable: Option<bool>,
    pub mountpoints: Vec<PathBuf>,
}

#[cfg(target_os = "linux")]
fn probe_linux(query: Option<&str>) -> Result<Vec<LsblkDevice>, FlashError> {
    let output = Command::new("lsblk")
        .args(["-J", "-O"])
        .output()
        .map_err(|err| FlashError::ProbeFailed(err.to_string()))?;
    if !output.status.success() {
        return Err(FlashError::ProbeFailed(format!(
            "lsblk failed with code {:?}",
            output.status.code()
        )));
    }
    let parsed: LsblkOutput = serde_json::from_slice(&output.stdout)
        .map_err(|err| FlashError::ProbeFailed(format!("parse lsblk output: {err}")))?;
    let mut devices = Vec::new();
    for device in parsed.blockdevices.into_iter().flatten() {
        flatten_lsblk(device, &mut devices);
    }
    let filtered = if let Some(query_str) = query {
        let matcher = Query::parse(query_str)?;
        devices
            .into_iter()
            .filter(|dev| matcher.matches(dev))
            .collect()
    } else {
        devices
    };
    Ok(filtered)
}

#[cfg(target_os = "linux")]
fn flatten_lsblk(device: RawLsblkDevice, out: &mut Vec<LsblkDevice>) {
    let mountpoints = device
        .mountpoints
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| m)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let dev_path = PathBuf::from("/dev").join(&device.name);
    out.push(LsblkDevice {
        name: device.name,
        dev_path,
        serial: device.serial,
        vendor: device.vendor,
        model: device.model,
        fs_type: device.fs_type,
        removable: device.rm,
        mountpoints,
    });
    if let Some(children) = device.children {
        for child in children {
            flatten_lsblk(child, out);
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Deserialize)]
struct LsblkOutput {
    blockdevices: Option<Vec<RawLsblkDevice>>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Deserialize)]
struct RawLsblkDevice {
    name: String,
    #[serde(default)]
    serial: Option<String>,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "fstype")]
    fs_type: Option<String>,
    #[serde(default, deserialize_with = "bool_from_any")]
    rm: Option<bool>,
    #[serde(default)]
    mountpoints: Option<Vec<Option<String>>>,
    #[serde(default)]
    children: Option<Vec<RawLsblkDevice>>,
}

#[cfg(target_os = "linux")]
fn bool_from_any<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Option<bool>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("bool or number")
        }
        fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(v))
        }
        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(v != 0))
        }
        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(v != 0))
        }
    }
    deserializer.deserialize_any(Visitor)
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct Query {
    clauses: Vec<QueryClause>,
}

#[cfg(target_os = "linux")]
impl Query {
    fn parse(input: &str) -> Result<Self, FlashError> {
        let mut clauses = Vec::new();
        for part in input.split("and") {
            let clause = QueryClause::parse(part.trim())?;
            clauses.push(clause);
        }
        Ok(Query { clauses })
    }

    fn matches(&self, device: &LsblkDevice) -> bool {
        self.clauses.iter().all(|c| c.matches(device))
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
enum QueryClause {
    Equals { field: QueryField, value: String },
    Regex { field: QueryField, regex: Regex },
}

#[cfg(target_os = "linux")]
impl QueryClause {
    fn parse(raw: &str) -> Result<Self, FlashError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(FlashError::InvalidQuery(raw.into()));
        }
        if let Some((field, value)) = raw.split_once("~=") {
            let field = QueryField::parse(field.trim())?;
            let regex = Regex::new(value.trim()).map_err(|_| FlashError::InvalidQuery(raw.into()))?;
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

    fn matches(&self, device: &LsblkDevice) -> bool {
        match self {
            QueryClause::Equals { field, value } => field.equals(device, value),
            QueryClause::Regex { field, regex } => field.regex(device, regex),
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
enum QueryField {
    Serial,
    Vendor,
    Model,
    FsType,
    Removable,
}

#[cfg(target_os = "linux")]
impl QueryField {
    fn parse(raw: &str) -> Result<Self, FlashError> {
        match raw.to_ascii_lowercase().as_str() {
            "serial" => Ok(QueryField::Serial),
            "vendor" => Ok(QueryField::Vendor),
            "model" => Ok(QueryField::Model),
            "fstype" | "fs_type" => Ok(QueryField::FsType),
            "removable" | "rm" => Ok(QueryField::Removable),
            _ => Err(FlashError::InvalidQuery(raw.into())),
        }
    }

    fn equals(&self, device: &LsblkDevice, value: &str) -> bool {
        match self {
            QueryField::Serial => device
                .serial
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::Vendor => device
                .vendor
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::Model => device
                .model
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::FsType => device
                .fs_type
                .as_deref()
                .map_or(false, |v| v.eq_ignore_ascii_case(value)),
            QueryField::Removable => device.removable.unwrap_or(false) == (value == "true"),
        }
    }

    fn regex(&self, device: &LsblkDevice, regex: &Regex) -> bool {
        match self {
            QueryField::Serial => device.serial.as_deref().map_or(false, |v| regex.is_match(v)),
            QueryField::Vendor => device.vendor.as_deref().map_or(false, |v| regex.is_match(v)),
            QueryField::Model => device.model.as_deref().map_or(false, |v| regex.is_match(v)),
            QueryField::FsType => device.fs_type.as_deref().map_or(false, |v| regex.is_match(v)),
            QueryField::Removable => regex.is_match(if device.removable.unwrap_or(false) {
                "true"
            } else {
                "false"
            }),
        }
    }
}
