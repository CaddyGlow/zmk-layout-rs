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

    let child = Command::new("diskutil")
        .args(["activity"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            FlashError::WatcherFailed(format!("failed to start diskutil activity: {err}"))
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

        if let Some(event) = parse_diskutil_activity_line(&line) {
            flash_debug(format!(
                "diskutil activity: kind={} identifier={}",
                event.kind, event.identifier
            ));

            match event.kind.as_str() {
                "appeared" | "mounted" => {
                    // Brief delay for mount to settle
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
                "disappeared" | "unmounted" => {
                    let _ = tx.send(FlashEvent::Removed(FlashId {
                        serial: None,
                        dev_path: Some(event.identifier),
                    }));
                }
                _ => {}
            }
        }
    }

    let _ = child.kill();
}

struct DiskutilActivityEvent {
    kind: String,
    identifier: String,
}

pub(super) fn parse_diskutil_activity_line(line: &str) -> Option<DiskutilActivityEvent> {
    // diskutil activity outputs lines like:
    //   ***DiskAppeared ('disk4s1', DAVolumePath = 'file:///Volumes/GLV80')
    //   ***DiskDisappeared ('disk4s1', DAVolumePath = '')
    //   ***DiskMountApproval ('disk4s1', DAVolumePath = 'file:///Volumes/GLV80')
    //   ***DiskUnmountApproval ('disk4s1', DAVolumePath = 'file:///Volumes/GLV80')
    let trimmed = line.trim();
    if !trimmed.starts_with("***Disk") {
        return None;
    }

    let after_stars = &trimmed[3..];
    let kind = if after_stars.starts_with("DiskAppeared") {
        "appeared"
    } else if after_stars.starts_with("DiskDisappeared") {
        "disappeared"
    } else if after_stars.starts_with("DiskMountApproval") {
        "mounted"
    } else if after_stars.starts_with("DiskUnmountApproval") {
        "unmounted"
    } else {
        return None;
    };

    // Extract identifier from parentheses: ('disk4s1', ...)
    let paren_start = after_stars.find('(')?;
    let content = &after_stars[paren_start + 1..];

    // Find the identifier between quotes
    let quote_start = content.find('\'')?;
    let rest = &content[quote_start + 1..];
    let quote_end = rest.find('\'')?;
    let identifier = rest[..quote_end].to_string();

    if identifier.is_empty() {
        return None;
    }

    Some(DiskutilActivityEvent {
        kind: kind.to_string(),
        identifier,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_disk_appeared() {
        let line =
            "***DiskAppeared ('disk4s1', DAVolumePath = 'file:///Volumes/GLV80')";
        let event = parse_diskutil_activity_line(line).unwrap();
        assert_eq!(event.kind, "appeared");
        assert_eq!(event.identifier, "disk4s1");
    }

    #[test]
    fn parse_disk_disappeared() {
        let line = "***DiskDisappeared ('disk4s1', DAVolumePath = '')";
        let event = parse_diskutil_activity_line(line).unwrap();
        assert_eq!(event.kind, "disappeared");
        assert_eq!(event.identifier, "disk4s1");
    }

    #[test]
    fn parse_disk_mount_approval() {
        let line =
            "***DiskMountApproval ('disk4s1', DAVolumePath = 'file:///Volumes/GLV80')";
        let event = parse_diskutil_activity_line(line).unwrap();
        assert_eq!(event.kind, "mounted");
        assert_eq!(event.identifier, "disk4s1");
    }

    #[test]
    fn parse_disk_unmount_approval() {
        let line =
            "***DiskUnmountApproval ('disk4s1', DAVolumePath = 'file:///Volumes/GLV80')";
        let event = parse_diskutil_activity_line(line).unwrap();
        assert_eq!(event.kind, "unmounted");
        assert_eq!(event.identifier, "disk4s1");
    }

    #[test]
    fn ignores_non_disk_lines() {
        assert!(parse_diskutil_activity_line("some other output").is_none());
        assert!(parse_diskutil_activity_line("").is_none());
    }

    #[test]
    fn ignores_unrecognized_disk_events() {
        assert!(parse_diskutil_activity_line("***DiskPeek ('disk4')").is_none());
        assert!(
            parse_diskutil_activity_line("***DiskDescriptionChanged ('disk4s1')").is_none()
        );
    }

    #[test]
    fn parse_disk_with_whitespace() {
        let line =
            "  ***DiskAppeared ('disk2s1', DAVolumePath = 'file:///Volumes/USB')  ";
        let event = parse_diskutil_activity_line(line).unwrap();
        assert_eq!(event.kind, "appeared");
        assert_eq!(event.identifier, "disk2s1");
    }

    #[test]
    fn parse_disk_whole_device() {
        let line = "***DiskAppeared ('disk4', DAVolumePath = '')";
        let event = parse_diskutil_activity_line(line).unwrap();
        assert_eq!(event.kind, "appeared");
        assert_eq!(event.identifier, "disk4");
    }
}
