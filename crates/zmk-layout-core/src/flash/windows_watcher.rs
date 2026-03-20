use serde_json;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use super::{
    core::{flash_debug, FlashConfig, FlashError},
    watcher::{ChannelWatcher, FlashEvent, FlashId, FlashWatcher},
};

pub(super) fn start_watcher(
    query: Option<&str>,
) -> Result<Box<dyn FlashWatcher>, FlashError> {
    let (tx, rx) = mpsc::channel();
    let query_str = query.map(String::from);

    let ps_script = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Register-WmiEvent -Class Win32_VolumeChangeEvent -SourceIdentifier VolumeChange
while ($true) {
    $event = Wait-Event -SourceIdentifier VolumeChange -Timeout 1
    if ($event) {
        $eventType = $event.SourceEventArgs.NewEvent.EventType
        $driveName = $event.SourceEventArgs.NewEvent.DriveName
        Remove-Event -SourceIdentifier VolumeChange
        $obj = @{ EventType = $eventType; DriveName = $driveName }
        $obj | ConvertTo-Json -Compress
    }
}
"#;

    let child = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_script])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            FlashError::WatcherFailed(format!(
                "failed to start PowerShell volume watcher: {err}"
            ))
        })?;

    thread::spawn(move || {
        run_watcher(child, tx, query_str);
    });

    Ok(Box::new(ChannelWatcher::new(rx)))
}

fn run_watcher(
    mut child: std::process::Child,
    tx: mpsc::Sender<FlashEvent>,
    query: Option<String>,
) {
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => return,
    };
    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(event) = parse_volume_change_event(trimmed) {
            flash_debug(format!(
                "powershell volume event: type={} drive={}",
                event.event_type,
                event.drive_name.as_deref().unwrap_or("-")
            ));

            match event.event_type {
                // EventType 2 = device arrival
                2 => {
                    // Brief delay for mount to settle
                    thread::sleep(Duration::from_millis(300));

                    let config = FlashConfig {
                        device_query: query.clone(),
                        ..Default::default()
                    };
                    if let Ok(devices) = super::platform::discover_devices(&config) {
                        for device in devices {
                            if tx.send(FlashEvent::Added(device)).is_err() {
                                return;
                            }
                        }
                    }
                }
                // EventType 3 = device removal
                3 => {
                    let _ = tx.send(FlashEvent::Removed(FlashId {
                        serial: None,
                        dev_path: event.drive_name,
                    }));
                }
                _ => {}
            }
        }
    }

    let _ = child.kill();
}

struct VolumeChangeEvent {
    event_type: u32,
    drive_name: Option<String>,
}

pub(super) fn parse_volume_change_event(json_line: &str) -> Option<VolumeChangeEvent> {
    let value: serde_json::Value = serde_json::from_str(json_line).ok()?;
    let event_type = value.get("EventType")?.as_u64()? as u32;
    let drive_name = value
        .get("DriveName")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some(VolumeChangeEvent {
        event_type,
        drive_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_device_arrival_event() {
        let json = r#"{"EventType":2,"DriveName":"E:"}"#;
        let event = parse_volume_change_event(json).unwrap();
        assert_eq!(event.event_type, 2);
        assert_eq!(event.drive_name.as_deref(), Some("E:"));
    }

    #[test]
    fn parse_device_removal_event() {
        let json = r#"{"EventType":3,"DriveName":"E:"}"#;
        let event = parse_volume_change_event(json).unwrap();
        assert_eq!(event.event_type, 3);
        assert_eq!(event.drive_name.as_deref(), Some("E:"));
    }

    #[test]
    fn parse_event_with_null_drive() {
        let json = r#"{"EventType":2,"DriveName":null}"#;
        let event = parse_volume_change_event(json).unwrap();
        assert_eq!(event.event_type, 2);
        assert!(event.drive_name.is_none());
    }

    #[test]
    fn parse_event_without_drive_key() {
        let json = r#"{"EventType":1}"#;
        let event = parse_volume_change_event(json).unwrap();
        assert_eq!(event.event_type, 1);
        assert!(event.drive_name.is_none());
    }

    #[test]
    fn parse_ignores_invalid_json() {
        assert!(parse_volume_change_event("not json").is_none());
        assert!(parse_volume_change_event("").is_none());
    }

    #[test]
    fn parse_ignores_missing_event_type() {
        assert!(parse_volume_change_event(r#"{"DriveName":"E:"}"#).is_none());
        assert!(parse_volume_change_event("{}").is_none());
    }

    #[test]
    fn parse_event_with_empty_drive() {
        let json = r#"{"EventType":2,"DriveName":""}"#;
        let event = parse_volume_change_event(json).unwrap();
        assert_eq!(event.event_type, 2);
        assert!(event.drive_name.is_none());
    }

    #[test]
    fn parse_config_change_event() {
        // EventType 1 = config changed
        let json = r#"{"EventType":1,"DriveName":"C:"}"#;
        let event = parse_volume_change_event(json).unwrap();
        assert_eq!(event.event_type, 1);
        assert_eq!(event.drive_name.as_deref(), Some("C:"));
    }
}
