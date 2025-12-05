use serde_json;
use std::{
    collections::HashSet,
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
    let devices = probe_windows(config.device_query.as_deref())?;
    Ok(devices.into_iter().map(FlashDiscovery::from).collect())
}

pub(super) fn wait_for_device(
    config: &FlashConfig,
    _target: Option<&FlashTarget>,
    seen_serials: &HashSet<String>,
) -> Result<FlashDevice, FlashError> {
    let deadline = Instant::now() + config.mount_timeout;
    let mut last_error = None;
    let mut attempts = 0;
    while Instant::now() < deadline {
        match probe_windows(config.device_query.as_deref()) {
            Ok(mut devices) => {
                attempts += 1;
                flash_debug(format!(
                    "probe attempt {} found {} windows devices (query={})",
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
                    let dev = devices.remove(0);
                    if dev.mountpoints.is_empty() {
                        last_error = Some(FlashError::UnmountedDevice { name: dev.name });
                    } else {
                        let mountpoint = dev.mountpoints[0].clone();
                        return Ok(FlashDevice {
                            name: dev.name,
                            dev_path: Some(dev.dev_path),
                            mountpoint,
                            serial: dev.serial,
                            vendor: dev.vendor,
                            model: dev.model,
                            fs_type: dev.fs_type,
                            auto_unmount: false,
                            cleanup_path: None,
                            vendor_id: None,
                            product_id: None,
                        });
                    }
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
                            auto_unmount: false,
                            cleanup_path: None,
                            vendor_id: None,
                            product_id: None,
                        });
                    }
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

fn probe_windows(query: Option<&str>) -> Result<Vec<WinVolume>, FlashError> {
    flash_debug("probing windows volumes via powershell Get-Volume");
    // Try to enrich volume data with disk metadata so vendor/model/serial queries can work.
    // Use the provided minimal USB script to align with other platforms.
    let ps_script = r#"$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$results = @()

Get-CimInstance Win32_DiskDrive | Where-Object InterfaceType -eq 'USB' | ForEach-Object {
    $disk = $_

    # Extract vendor, serial from disk's PNPDeviceID
    $vendor = if ($disk.PNPDeviceID -match 'VEN_([^&]+)') { $matches[1] } else { $disk.Manufacturer }
    $serial = if ($disk.PNPDeviceID -match 'REV_\\([^\\&]+)') { $matches[1] } else { 'Unknown' }

    # Find the parent USB device using the serial number
    $usbDevice = Get-CimInstance Win32_USBHub | Where-Object {
        $_.DeviceID -like \"*$serial*\"
    } | Select-Object -First 1

    # Extract VID and PID from USB device
    if ($usbDevice) {
        $vendorId = if ($usbDevice.DeviceID -match 'VID_([0-9A-F]+)') { $matches[1] } else { 'Unknown' }
        $productId = if ($usbDevice.DeviceID -match 'PID_([0-9A-F]+)') { $matches[1] } else { 'Unknown' }
    } else {
        $vendorId = 'Unknown'
        $productId = 'Unknown'
    }

    # Get mount points
    $partitions = Get-CimInstance -Query \"ASSOCIATORS OF {Win32_DiskDrive.DeviceID='$($disk.DeviceID)'} WHERE AssocClass=Win32_DiskDriveToDiskPartition\"
    $mountpoints = @()
    foreach ($partition in $partitions) {
        $logicalDisks = Get-CimInstance -Query \"ASSOCIATORS OF {Win32_DiskPartition.DeviceID='$($partition.DeviceID)'} WHERE AssocClass=Win32_LogicalDiskToPartition\"
        $mountpoints += $logicalDisks | ForEach-Object { $_.DeviceID }
    }

    $results += [PSCustomObject]@{
        Name        = $disk.Caption
        Model       = $disk.Model
        Vendor      = $vendor
        VID         = $vendorId
        PID         = $productId
        Serial      = $serial
        Mountpoints = $mountpoints
        Removable   = $disk.MediaType -eq 'Removable Media'
    }
}

if (-not $results -or $results.Count -eq 0) { '[]' } else { $results | ConvertTo-Json -Compress }"#;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_script])
        .output()
        .map_err(|err| FlashError::ProbeFailed(err.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    flash_debug(format!(
        "powershell Get-Volume exit_code={:?} stdout_len={} stderr_len={}",
        output.status.code(),
        stdout.as_ref().len(),
        stderr.as_ref().len()
    ));
    if !stderr.trim().is_empty() {
        flash_debug(format!("powershell Get-Volume stderr: {}", stderr.trim()));
    }
    if !output.status.success() {
        return Err(FlashError::ProbeFailed(format!(
            "powershell Get-Volume failed with code {:?}",
            output.status.code()
        )));
    }
    flash_debug(format!("powershell Get-Volume stdout: {}", stdout.trim()));
    let value: serde_json::Value = serde_json::from_str(stdout.as_ref())
        .map_err(|err| FlashError::ProbeFailed(format!("parse Get-Volume output: {err}")))?;
    let mut volumes = Vec::new();
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(vol) = parse_windows_volume(&item) {
                    volumes.push(vol);
                }
            }
        }
        serde_json::Value::Object(_) => {
            if let Some(vol) = parse_windows_volume(&value) {
                volumes.push(vol);
            }
        }
        _ => {}
    }
    log_windows_volumes("after parse", &volumes);
    if let Some(query_str) = query {
        let matcher = Query::parse(query_str)?;
        let before = volumes.len();
        volumes.retain(|v| matcher.matches(&QueryMetadata::from(v)));
        flash_debug(format!(
            "windows volume query `{}` retained {} of {} devices",
            query_str,
            volumes.len(),
            before
        ));
        log_windows_volumes("after filter", &volumes);
    }
    Ok(volumes)
}

fn parse_windows_volume(value: &serde_json::Value) -> Option<WinVolume> {
    let clean = |s: Option<String>| {
        s.map(|v| v.trim().trim_end_matches('.').to_string())
            .filter(|v| {
                let lower = v.to_ascii_lowercase();
                !v.is_empty() && v != "0" && lower != "unknown"
            })
    };

    let name = clean(
        value
            .get("Name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    )
    .unwrap_or_else(|| "volume".into());
    let model = clean(
        value
            .get("Model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    );
    let vendor = clean(
        value
            .get("Vendor")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    );
    let serial = clean(
        value
            .get("Serial")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    );
    let removable = value
        .get("Removable")
        .and_then(|v| v.as_bool())
        .or_else(|| {
            value
                .get("Removable")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case("true"))
        });

    let mut mountpoints = Vec::new();
    if let Some(mps) = value.get("Mountpoints").and_then(|v| v.as_array()) {
        for mp in mps {
            if let Some(s) = mp.as_str() {
                let trimmed = s.trim().trim_end_matches('\\');
                if trimmed.is_empty() {
                    continue;
                }
                let path = if trimmed.ends_with(':') {
                    format!(r"{}\\", trimmed)
                } else {
                    format!(r"{}:\\", trimmed)
                };
                mountpoints.push(PathBuf::from(path));
            }
        }
    }
    if mountpoints.is_empty() {
        return None;
    }
    let dev_path = mountpoints
        .get(0)
        .cloned()
        .unwrap_or_else(|| PathBuf::from(name.clone()));
    flash_debug(format!(
        "windows volume parsed name={} model={:?} vendor={:?} serial={:?} mountpoints={:?} removable={:?}",
        name, model, vendor, serial, mountpoints, removable
    ));
    Some(WinVolume {
        name,
        dev_path,
        mountpoints,
        serial,
        vendor,
        model,
        fs_type: None,
        removable,
    })
}

#[derive(Debug, Clone)]
struct WinVolume {
    pub name: String,
    pub dev_path: PathBuf,
    pub mountpoints: Vec<PathBuf>,
    pub serial: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub fs_type: Option<String>,
    pub removable: Option<bool>,
}

impl From<&WinVolume> for QueryMetadata {
    fn from(device: &WinVolume) -> Self {
        QueryMetadata {
            serial: device.serial.clone(),
            vendor: device.vendor.clone(),
            vendor_id: None,
            model: device.model.clone(),
            product_id: None,
            fs_type: device.fs_type.clone(),
            removable: device.removable,
        }
    }
}

impl From<WinVolume> for FlashDiscovery {
    fn from(device: WinVolume) -> Self {
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

fn log_windows_volumes(label: &str, volumes: &[WinVolume]) {
    flash_debug(format!("windows volumes {label}: {} found", volumes.len()));
    for volume in volumes {
        flash_debug(format!(
            "  name={} dev_path={} vendor={} model={} serial={} mountpoints={:?} fs_type={:?} removable={:?}",
            volume.name,
            volume.dev_path.display(),
            volume.vendor.as_deref().unwrap_or("-"),
            volume.model.as_deref().unwrap_or("-"),
            volume.serial.as_deref().unwrap_or("-"),
            volume.mountpoints,
            volume.fs_type.clone(),
            volume.removable
        ));
    }
}
