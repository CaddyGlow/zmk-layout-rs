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

    let child = Command::new("udevadm")
        .args(["monitor", "--udev", "--subsystem-match=block"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            FlashError::WatcherFailed(format!("failed to start udevadm monitor: {err}"))
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

        if let Some(event) = parse_udevadm_line(&line) {
            flash_debug(format!(
                "udevadm event: action={} device={}",
                event.action, event.device_name
            ));

            match event.action.as_str() {
                "add" | "change" => {
                    // Brief delay for automount to settle
                    thread::sleep(Duration::from_millis(200));

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
                "remove" => {
                    let _ = tx.send(FlashEvent::Removed(FlashId {
                        serial: None,
                        dev_path: Some(format!("/dev/{}", event.device_name)),
                    }));
                }
                _ => {}
            }
        }
    }

    let _ = child.kill();
}

pub(super) struct UdevadmEvent {
    action: String,
    device_name: String,
}

pub(super) fn parse_udevadm_line(line: &str) -> Option<UdevadmEvent> {
    // Format: UDEV  [timestamp] action      /devices/.../block/sdX (block)
    if !line.starts_with("UDEV") {
        return None;
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    // Timestamp must be in brackets: [1234.5]
    if !parts[1].starts_with('[') || !parts[1].ends_with(']') {
        return None;
    }
    let action = parts[2].to_string();
    let syspath = parts[3];
    // syspath must look like an absolute path
    if !syspath.starts_with('/') {
        return None;
    }
    let device_name = syspath.rsplit('/').next()?.to_string();
    if device_name.is_empty() {
        return None;
    }
    Some(UdevadmEvent {
        action,
        device_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_udevadm_add_event() {
        let line = "UDEV  [1234567890.123456] add      /devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0/host0/target0:0:0/0:0:0:0/block/sda/sda1 (block)";
        let event = parse_udevadm_line(line).unwrap();
        assert_eq!(event.action, "add");
        assert_eq!(event.device_name, "sda1");
    }

    #[test]
    fn parse_udevadm_remove_event() {
        let line = "UDEV  [1234567890.234567] remove   /devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0/host0/target0:0:0/0:0:0:0/block/sda (block)";
        let event = parse_udevadm_line(line).unwrap();
        assert_eq!(event.action, "remove");
        assert_eq!(event.device_name, "sda");
    }

    #[test]
    fn parse_udevadm_change_event() {
        let line = "UDEV  [1234567890.345678] change   /devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0/host0/target0:0:0/0:0:0:0/block/sda1 (block)";
        let event = parse_udevadm_line(line).unwrap();
        assert_eq!(event.action, "change");
        assert_eq!(event.device_name, "sda1");
    }

    #[test]
    fn parse_udevadm_ignores_header() {
        assert!(parse_udevadm_line("monitor will print the received events for:").is_none());
        assert!(parse_udevadm_line("UDEV - the event which udev sends out after rule processing").is_none());
    }

    #[test]
    fn parse_udevadm_ignores_kernel_events() {
        let line = "KERNEL  [1234567890.123456] add      /devices/block/sda1 (block)";
        assert!(parse_udevadm_line(line).is_none());
    }

    #[test]
    fn parse_udevadm_ignores_short_lines() {
        assert!(parse_udevadm_line("UDEV").is_none());
        assert!(parse_udevadm_line("UDEV  [ts]").is_none());
        assert!(parse_udevadm_line("UDEV  [ts] add").is_none());
    }

    #[test]
    fn parse_udevadm_handles_nested_device_path() {
        let line = "UDEV  [12345.6] add      /devices/platform/soc/usb/block/mmcblk0p1 (block)";
        let event = parse_udevadm_line(line).unwrap();
        assert_eq!(event.action, "add");
        assert_eq!(event.device_name, "mmcblk0p1");
    }

    #[test]
    fn parse_udevadm_parenthesized_subsystem_ignored() {
        // The (block) part at the end is a separate token, not part of the path
        let line = "UDEV  [12345.6] add      /devices/block/sda1 (block)";
        let event = parse_udevadm_line(line).unwrap();
        assert_eq!(event.device_name, "sda1");
    }
}
