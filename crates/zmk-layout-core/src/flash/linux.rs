use serde::Deserialize;
use serde_json;
use std::{
    collections::HashSet,
    path::PathBuf,
    process::Command,
    thread::sleep,
    time::{Duration, Instant},
};

use super::core::{
    FlashConfig, FlashDevice, FlashDiscovery, FlashError, FlashTarget, Query, QueryMetadata,
    flash_debug,
};

pub(super) fn discover_devices(config: &FlashConfig) -> Result<Vec<FlashDiscovery>, FlashError> {
    let devices = probe_linux(config.device_query.as_deref())?;
    Ok(devices.into_iter().map(FlashDiscovery::from).collect())
}

pub(super) fn wait_for_device(
    config: &FlashConfig,
    _target: Option<&FlashTarget>,
    seen_serials: &HashSet<String>,
) -> Result<FlashDevice, FlashError> {
    wait_for_device_linux(config, seen_serials)
}

fn wait_for_device_linux(
    config: &FlashConfig,
    seen_serials: &HashSet<String>,
) -> Result<FlashDevice, FlashError> {
    let deadline = Instant::now() + config.mount_timeout;
    let mut last_error = None;
    let mut attempts = 0;
    while Instant::now() < deadline {
        match probe_linux(config.device_query.as_deref()) {
            Ok(mut devices) => {
                attempts += 1;
                flash_debug(format!(
                    "probe attempt {} found {} linux devices (query={})",
                    attempts,
                    devices.len(),
                    config.device_query.as_deref().unwrap_or("<none>")
                ));
                if !seen_serials.is_empty() {
                    let previous = devices.len();
                    devices.retain(|d| {
                        d.serial
                            .as_ref()
                            .map(|s| !seen_serials.contains(s))
                            .unwrap_or(true)
                    });
                    if devices.is_empty() && previous > 0 {
                        last_error = Some(FlashError::DuplicateSerial {
                            serial: "<already flashed>".into(),
                        });
                    }
                }
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
                        vendor_id: dev.vendor_id,
                        product_id: dev.product_id,
                    });
                }
                if devices.len() > 1 {
                    // Prefer a device with a mountpoint to avoid extra mounts; otherwise pick the first.
                    if let Some(dev) = devices.iter().find(|d| !d.mountpoints.is_empty()) {
                        let dev = dev.clone();
                        let mountpoint = dev.mountpoints[0].clone();
                        return Ok(FlashDevice {
                            name: dev.name.clone(),
                            dev_path: Some(dev.dev_path.clone()),
                            mountpoint,
                            serial: dev.serial.clone(),
                            vendor: dev.vendor.clone(),
                            model: dev.model.clone(),
                            fs_type: dev.fs_type.clone(),
                            auto_unmount: false,
                            cleanup_path: None,
                            vendor_id: dev.vendor_id.clone(),
                            product_id: dev.product_id.clone(),
                        });
                    }
                    let dev = devices.remove(0);
                    let mount = mount_with_udisksctl(&dev.dev_path)?;
                    return Ok(FlashDevice {
                        name: dev.name,
                        dev_path: Some(dev.dev_path),
                        mountpoint: mount,
                        serial: dev.serial,
                        vendor: dev.vendor,
                        model: dev.model,
                        fs_type: dev.fs_type,
                        auto_unmount: true,
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

fn mount_with_udisksctl(dev_path: &PathBuf) -> Result<PathBuf, FlashError> {
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

#[derive(Debug, Clone)]
struct LsblkDevice {
    pub name: String,
    pub dev_path: PathBuf,
    pub serial: Option<String>,
    pub vendor: Option<String>,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
    pub model: Option<String>,
    pub fs_type: Option<String>,
    pub removable: Option<bool>,
    pub mountpoints: Vec<PathBuf>,
}

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
    let devices = parse_lsblk_devices(&output.stdout)?;
    let filtered = if let Some(query_str) = query {
        let matcher = Query::parse(query_str)?;
        devices
            .into_iter()
            .filter(|dev| matcher.matches(&QueryMetadata::from(dev)))
            .collect()
    } else {
        devices
    };
    Ok(filtered)
}

fn parse_lsblk_devices(bytes: &[u8]) -> Result<Vec<LsblkDevice>, FlashError> {
    let parsed: LsblkOutput = serde_json::from_slice(bytes)
        .map_err(|err| FlashError::ProbeFailed(format!("parse lsblk output: {err}")))?;
    let mut devices = Vec::new();
    for device in parsed.blockdevices.into_iter().flatten() {
        flatten_lsblk(device, &mut devices);
    }
    Ok(devices)
}

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
        vendor_id: device.vendor_id,
        product_id: device.product_id,
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

#[derive(Debug, Deserialize)]
struct LsblkOutput {
    blockdevices: Option<Vec<RawLsblkDevice>>,
}

#[derive(Debug, Deserialize)]
struct RawLsblkDevice {
    name: String,
    #[serde(default)]
    serial: Option<String>,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    vendor_id: Option<String>,
    #[serde(default)]
    product_id: Option<String>,
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

impl From<&LsblkDevice> for QueryMetadata {
    fn from(device: &LsblkDevice) -> Self {
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

impl From<LsblkDevice> for FlashDiscovery {
    fn from(device: LsblkDevice) -> Self {
        FlashDiscovery {
            name: device.name,
            dev_path: Some(device.dev_path),
            mountpoints: device.mountpoints,
            serial: device.serial,
            vendor: device.vendor,
            model: device.model,
            fs_type: device.fs_type,
            removable: device.removable,
            vendor_id: device.vendor_id,
            product_id: device.product_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsblk_and_query_filters() {
        let sample = br#"{
            "blockdevices": [
                {
                    "name": "sda",
                    "serial": "ATA-123",
                    "vendor": "ATA",
                    "model": "Drive",
                    "fstype": null,
                    "rm": false,
                    "mountpoints": [null],
                    "children": [
                        {
                            "name": "sda1",
                            "serial": "GLV80-ABC123",
                            "vendor": "MoErgo",
                            "model": "Glove80 Boot",
                            "fstype": "vfat",
                            "rm": true,
                            "mountpoints": [
                                "/media/user/GLV80"
                            ]
                        }
                    ]
                }
            ]
        }"#;
        let devices = parse_lsblk_devices(sample).expect("parse lsblk");
        assert_eq!(devices.len(), 2);
        let boot = devices
            .iter()
            .find(|d| d.name == "sda1")
            .expect("boot device present");
        assert_eq!(
            boot.mountpoints.first().map(|p| p.to_str().unwrap()),
            Some("/media/user/GLV80")
        );
        assert_eq!(boot.dev_path, PathBuf::from("/dev/sda1"));

        let query = Query::parse("serial~=GLV80-.* and removable=true").unwrap();
        let meta = QueryMetadata::from(boot);
        assert!(query.matches(&meta));
    }

    #[test]
    fn parses_udisksctl_mount_output() {
        let output = "Mounted /dev/sda1 at /media/user/GLV80.\n";
        let mount = parse_udisk_mount_path(output).unwrap();
        assert_eq!(mount, PathBuf::from("/media/user/GLV80"));
    }
}
