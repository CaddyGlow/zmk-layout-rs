use plist;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::PathBuf,
    process::Command,
    thread::sleep,
    time::{Duration, Instant},
};

use super::core::{
    flash_debug, FlashConfig, FlashDevice, FlashDiscovery, FlashError, FlashTarget, Query,
    QueryMetadata,
};

pub(super) fn discover_devices(config: &FlashConfig) -> Result<Vec<FlashDiscovery>, FlashError> {
    let devices = probe_macos(config.device_query.as_deref())?;
    Ok(devices.into_iter().map(FlashDiscovery::from).collect())
}

pub(super) fn wait_for_device(
    config: &FlashConfig,
    _target: Option<&FlashTarget>,
    seen_serials: &HashSet<String>,
) -> Result<FlashDevice, FlashError> {
    wait_for_device_macos(config, seen_serials)
}

#[derive(Debug, Clone, Default)]
pub(super) struct UsbDeviceInfo {
    pub(super) vendor: Option<String>,
    pub(super) product: Option<String>,
    pub(super) serial: Option<String>,
    pub(super) removable: Option<bool>,
    pub(super) vendor_id: Option<String>,
    pub(super) product_id: Option<String>,
}

pub(super) fn usb_device_metadata() -> Option<HashMap<String, UsbDeviceInfo>> {
    let output = Command::new("system_profiler")
        .args(["-json", "SPUSBDataType"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: Value = serde_json::from_slice(&output.stdout).ok()?;
    let mut map = HashMap::new();
    if let Some(items) = json.get("SPUSBDataType").and_then(|v| v.as_array()) {
        for item in items {
            walk_usb_items(item, &mut map);
        }
    }
    Some(map)
}

fn walk_usb_items(value: &Value, map: &mut HashMap<String, UsbDeviceInfo>) {
    let vendor = value
        .get("manufacturer")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let product = value
        .get("_name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let vendor_id = value
        .get("vendor_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let product_id = value
        .get("product_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let serial = value
        .get("serial_num")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    if let Some(media) = value.get("Media").and_then(|v| v.as_array()) {
        for item in media {
            let removable = item
                .get("removable_media")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case("yes") || s.eq_ignore_ascii_case("true"));
            if let Some(bsd) = item.get("bsd_name").and_then(|v| v.as_str()) {
                map.insert(
                    bsd.to_string(),
                    UsbDeviceInfo {
                        vendor: vendor.clone(),
                        product: product.clone(),
                        serial: serial.clone(),
                        removable,
                        vendor_id: vendor_id.clone(),
                        product_id: product_id.clone(),
                    },
                );
            }
        }
    }
    if let Some(children) = value.get("_items").and_then(|v| v.as_array()) {
        for child in children {
            walk_usb_items(child, map);
        }
    }
}

fn wait_for_device_macos(
    config: &FlashConfig,
    seen_serials: &HashSet<String>,
) -> Result<FlashDevice, FlashError> {
    let deadline = Instant::now() + config.mount_timeout;
    let mut last_error = None;
    let mut attempts = 0;
    while Instant::now() < deadline {
        match probe_macos(config.device_query.as_deref()) {
            Ok(mut devices) => {
                attempts += 1;
                flash_debug(format!(
                    "probe attempt {} found {} macOS devices (query={})",
                    attempts,
                    devices.len(),
                    config.device_query.as_deref().unwrap_or("<none>")
                ));
                if !seen_serials.is_empty() {
                    devices.retain(|d| {
                        d.serial
                            .as_ref()
                            .map(|s| !seen_serials.contains(s))
                            .unwrap_or(true)
                    });
                }
                if devices.len() == 1 {
                    let mut dev = devices.remove(0);
                    if dev.mountpoints.is_empty() {
                        let mount = diskutil_mount(&dev.dev_path)?;
                        dev.mountpoints.push(mount);
                        dev.auto_unmount = true;
                    }
                    let mountpoint = dev.mountpoints[0].clone();
                    return Ok(FlashDevice {
                        name: dev.name,
                        dev_path: Some(dev.dev_path),
                        mountpoint,
                        serial: dev.serial,
                        vendor: dev.vendor,
                        model: dev.model,
                        fs_type: dev.fs_type,
                        auto_unmount: dev.auto_unmount,
                        cleanup_path: None,
                        vendor_id: dev.vendor_id,
                        product_id: dev.product_id,
                    });
                }
                if devices.len() > 1 {
                    if let Some(dev) = devices.iter().find(|d| !d.mountpoints.is_empty()) {
                        let dev = dev.clone();
                        let mountpoint = dev.mountpoints[0].clone();
                        return Ok(FlashDevice {
                            name: dev.name,
                            dev_path: Some(dev.dev_path),
                            mountpoint,
                            serial: dev.serial,
                            vendor: dev.vendor,
                            model: dev.model,
                            fs_type: dev.fs_type,
                            auto_unmount: dev.auto_unmount,
                            cleanup_path: None,
                            vendor_id: dev.vendor_id,
                            product_id: dev.product_id,
                        });
                    }
                    let mut dev = devices.remove(0);
                    let mount = diskutil_mount(&dev.dev_path)?;
                    dev.mountpoints.push(mount.clone());
                    dev.auto_unmount = true;
                    return Ok(FlashDevice {
                        name: dev.name,
                        dev_path: Some(dev.dev_path),
                        mountpoint: mount,
                        serial: dev.serial,
                        vendor: dev.vendor,
                        model: dev.model,
                        fs_type: dev.fs_type,
                        auto_unmount: dev.auto_unmount,
                        cleanup_path: None,
                        vendor_id: dev.vendor_id,
                        product_id: dev.product_id,
                    });
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

fn probe_macos(query: Option<&str>) -> Result<Vec<MacDisk>, FlashError> {
    let output = Command::new("diskutil")
        .args(["list", "-plist"])
        .output()
        .map_err(|err| FlashError::ProbeFailed(err.to_string()))?;
    if !output.status.success() {
        return Err(FlashError::ProbeFailed(format!(
            "diskutil failed with code {:?}",
            output.status.code()
        )));
    }
    let parsed: DiskutilList = plist::from_bytes(&output.stdout)
        .map_err(|err| FlashError::ProbeFailed(format!("parse diskutil output: {err}")))?;
    let mut devices = Vec::new();
    for entry in parsed.all_devices.into_iter().flatten() {
        flatten_diskutil(entry, &mut devices);
    }
    log_macos_devices("before filter", &devices);
    if devices
        .iter()
        .any(|d| d.vendor.is_none() || d.serial.is_none())
    {
        enrich_from_usb_metadata(&mut devices);
        log_macos_devices("after usb enrichment", &devices);
    }
    if let Some(query_str) = query {
        let matcher = Query::parse(query_str)?;
        devices.retain(|dev| matcher.matches(&QueryMetadata::from(dev)));
        log_macos_devices("after filter", &devices);
    }
    Ok(devices)
}

fn enrich_from_usb_metadata(devices: &mut [MacDisk]) {
    if let Some(usb_info) = usb_device_metadata() {
        for dev in devices {
            if let Some(name) = dev.dev_path.file_name().and_then(OsStr::to_str) {
                if let Some(info) = usb_info.get(name) {
                    if dev.vendor.is_none() {
                        dev.vendor = info.vendor.clone();
                    }
                    if dev.model.is_none() {
                        dev.model = info.product.clone();
                    }
                    if dev
                        .serial
                        .as_deref()
                        .map_or(true, |s| s.eq_ignore_ascii_case("no name"))
                    {
                        dev.serial = info.serial.clone();
                    }
                    if dev.removable.is_none() {
                        dev.removable = info.removable;
                    }
                    if dev.vendor_id.is_none() {
                        dev.vendor_id = info.vendor_id.clone();
                    }
                    if dev.product_id.is_none() {
                        dev.product_id = info.product_id.clone();
                    }
                    if dev.serial.is_none() && info.vendor_id.is_some() && info.product_id.is_some()
                    {
                        dev.serial = Some(format!(
                            "{}:{}",
                            info.vendor_id.as_deref().unwrap_or(""),
                            info.product_id.as_deref().unwrap_or("")
                        ));
                    }
                }
            }
        }
    }
}

fn log_macos_devices(label: &str, devices: &[MacDisk]) {
    flash_debug(format!("macOS devices {label}: {} found", devices.len()));
    for dev in devices {
        flash_debug(format!(
            "  name={} model={} vendor={} serial={} vid={} pid={} mountpoints={:?} removable={:?} fs_type={:?}",
            dev.name,
            dev.model.as_deref().unwrap_or("-"),
            dev.vendor.as_deref().unwrap_or("-"),
            dev.serial.as_deref().unwrap_or("-"),
            dev.vendor_id.as_deref().unwrap_or("-"),
            dev.product_id.as_deref().unwrap_or("-"),
            dev.mountpoints,
            dev.removable,
            dev.fs_type
        ));
    }
}

fn diskutil_mount(dev_path: &PathBuf) -> Result<PathBuf, FlashError> {
    let dev_str = dev_path.to_str().unwrap_or_default();
    let output = Command::new("diskutil")
        .args(["mount", dev_str])
        .output()
        .map_err(|err| FlashError::ProbeFailed(err.to_string()))?;
    if !output.status.success() {
        return Err(FlashError::ProbeFailed(format!(
            "diskutil mount failed for {dev_str}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if let Some(info) = diskutil_info(dev_path)? {
        if let Some(mount) = info.mountpoint {
            return Ok(mount.into());
        }
    }
    Err(FlashError::ProbeFailed(format!(
        "unable to determine mountpoint for {dev_str}"
    )))
}

fn diskutil_info(dev_path: &PathBuf) -> Result<Option<DiskutilInfo>, FlashError> {
    let dev_str = dev_path.to_str().unwrap_or_default();
    let output = Command::new("diskutil")
        .args(["info", "-plist", dev_str])
        .output()
        .map_err(|err| FlashError::ProbeFailed(err.to_string()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let info: DiskutilInfo = plist::from_bytes(&output.stdout)
        .map_err(|err| FlashError::ProbeFailed(format!("parse diskutil info: {err}")))?;
    Ok(Some(info))
}

#[derive(Debug, Deserialize)]
struct DiskutilList {
    #[serde(rename = "AllDisksAndPartitions", default)]
    all_devices: Option<Vec<DiskutilEntry>>,
}

#[derive(Debug, Deserialize, Clone)]
struct DiskutilEntry {
    #[serde(rename = "DeviceIdentifier")]
    device_identifier: Option<String>,
    #[serde(rename = "VolumeName")]
    volume_name: Option<String>,
    #[serde(rename = "MountPoint")]
    mount_point: Option<String>,
    #[serde(rename = "RemovableMedia")]
    removable: Option<bool>,
    #[serde(rename = "Content")]
    content: Option<String>,
    #[serde(rename = "MediaName")]
    media_name: Option<String>,
    #[serde(rename = "Partitions", default)]
    partitions: Vec<DiskutilEntry>,
}

#[derive(Debug, Deserialize)]
struct DiskutilInfo {
    #[serde(rename = "MountPoint")]
    mountpoint: Option<String>,
    #[serde(rename = "FilesystemName")]
    fs_type: Option<String>,
    #[serde(rename = "VolumeName")]
    volume_name: Option<String>,
    #[serde(rename = "RemovableMedia")]
    removable: Option<bool>,
}

#[derive(Debug, Clone)]
struct MacDisk {
    pub name: String,
    pub dev_path: PathBuf,
    pub serial: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
    pub fs_type: Option<String>,
    pub removable: Option<bool>,
    pub mountpoints: Vec<PathBuf>,
    pub auto_unmount: bool,
}

fn flatten_diskutil(entry: DiskutilEntry, out: &mut Vec<MacDisk>) {
    if let Some(identifier) = entry.device_identifier.clone() {
        let mut mountpoints = Vec::new();
        if let Some(mp) = entry.mount_point {
            if !mp.is_empty() {
                mountpoints.push(PathBuf::from(mp));
            }
        }
        let dev_path = PathBuf::from("/dev").join(&identifier);
        out.push(MacDisk {
            name: identifier,
            dev_path,
            serial: entry.volume_name.clone().or(entry.media_name.clone()),
            vendor: None,
            model: entry.media_name.clone(),
            vendor_id: None,
            product_id: None,
            fs_type: entry.content.clone(),
            removable: entry.removable,
            mountpoints,
            auto_unmount: false,
        });
    }
    for child in entry.partitions {
        flatten_diskutil(child, out);
    }
}

impl From<&MacDisk> for QueryMetadata {
    fn from(device: &MacDisk) -> Self {
        QueryMetadata {
            serial: device.serial.clone(),
            vendor: device.vendor.clone(),
            vendor_id: device.vendor_id.clone(),
            model: device.model.clone(),
            product_id: device.product_id.clone(),
            fs_type: device.fs_type.clone(),
            removable: device.removable,
        }
    }
}

impl From<MacDisk> for FlashDiscovery {
    fn from(device: MacDisk) -> Self {
        FlashDiscovery {
            name: device.name,
            dev_path: Some(device.dev_path),
            mountpoints: device.mountpoints,
            serial: device.serial,
            vendor: device.vendor,
            model: device.model,
            fs_type: device.fs_type,
            removable: device.removable,
            vendor_id: None,
            product_id: None,
        }
    }
}
