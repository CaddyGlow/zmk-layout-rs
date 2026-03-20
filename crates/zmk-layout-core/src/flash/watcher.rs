use std::{
    collections::HashSet,
    io,
    sync::mpsc,
    time::Instant,
};

use super::core::{flash_debug, FlashConfig, FlashDevice, FlashDiscovery, FlashError, FlashTarget};

/// Lightweight identifier for a flash device, used in removal events.
#[derive(Debug, Clone)]
pub struct FlashId {
    pub serial: Option<String>,
    pub dev_path: Option<String>,
}

/// Event emitted by a flash device watcher.
#[derive(Debug, Clone)]
pub enum FlashEvent {
    /// A device matching the query appeared or became ready.
    Added(FlashDiscovery),
    /// A previously seen device was removed.
    Removed(FlashId),
}

/// Trait for platform-specific device watchers.
pub trait FlashWatcher: Send {
    /// Wait for the next device event, up to the given deadline.
    /// Returns `Ok(Some(event))` on event, `Ok(None)` on timeout, `Err` on failure.
    fn next_event(&mut self, until: Instant) -> io::Result<Option<FlashEvent>>;
}

/// Channel-based watcher used by all platform implementations.
#[allow(dead_code)]
pub(crate) struct ChannelWatcher {
    rx: mpsc::Receiver<FlashEvent>,
}

impl ChannelWatcher {
    #[allow(dead_code)]
    pub(crate) fn new(rx: mpsc::Receiver<FlashEvent>) -> Self {
        Self { rx }
    }
}

impl FlashWatcher for ChannelWatcher {
    fn next_event(&mut self, until: Instant) -> io::Result<Option<FlashEvent>> {
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        match self.rx.recv_timeout(remaining) {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        }
    }
}

/// Wait for a device using event-driven detection.
pub(crate) fn wait_for_device_events(
    config: &FlashConfig,
    _target: Option<&FlashTarget>,
    seen_serials: &HashSet<String>,
) -> Result<FlashDevice, FlashError> {
    let mut watcher = super::platform_watcher::start_watcher(config.device_query.as_deref())?;
    let deadline = Instant::now() + config.mount_timeout;

    flash_debug("waiting for device via event-driven detection");

    loop {
        if Instant::now() >= deadline {
            break;
        }

        match watcher.next_event(deadline) {
            Ok(Some(FlashEvent::Added(discovery))) => {
                flash_debug(format!(
                    "watcher: device added name={} serial={} mountpoints={:?}",
                    discovery.name,
                    discovery.serial.as_deref().unwrap_or("-"),
                    discovery.mountpoints
                ));

                if let Some(serial) = &discovery.serial {
                    if seen_serials.contains(serial) {
                        flash_debug(format!(
                            "watcher: skipping already-seen serial {serial}"
                        ));
                        continue;
                    }
                }

                if discovery.mountpoints.is_empty() {
                    flash_debug(format!(
                        "watcher: device {} has no mountpoints, skipping",
                        discovery.name
                    ));
                    continue;
                }

                let mountpoint = discovery.mountpoints[0].clone();
                return Ok(FlashDevice {
                    name: discovery.name,
                    dev_path: discovery.dev_path,
                    mountpoint,
                    serial: discovery.serial,
                    vendor: discovery.vendor,
                    model: discovery.model,
                    fs_type: discovery.fs_type,
                    auto_unmount: false,
                    cleanup_path: None,
                    vendor_id: discovery.vendor_id,
                    product_id: discovery.product_id,
                });
            }
            Ok(Some(FlashEvent::Removed(id))) => {
                flash_debug(format!(
                    "watcher: device removed dev_path={}",
                    id.dev_path.as_deref().unwrap_or("-")
                ));
            }
            Ok(None) => {}
            Err(err) => return Err(FlashError::WatcherFailed(err.to_string())),
        }
    }

    Err(FlashError::NoMatchingDevice(
        config
            .device_query
            .clone()
            .unwrap_or_else(|| "<unspecified>".into()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn channel_watcher_receives_event() {
        let (tx, rx) = mpsc::channel();
        let mut watcher = ChannelWatcher::new(rx);

        let discovery = FlashDiscovery {
            name: "test".into(),
            dev_path: Some(PathBuf::from("/dev/sda1")),
            mountpoints: vec![PathBuf::from("/mnt/test")],
            serial: Some("SER123".into()),
            vendor: None,
            model: None,
            fs_type: None,
            removable: None,
            vendor_id: None,
            product_id: None,
        };
        tx.send(FlashEvent::Added(discovery)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        let event = watcher.next_event(deadline).unwrap();
        assert!(matches!(event, Some(FlashEvent::Added(_))));
    }

    #[test]
    fn channel_watcher_returns_none_on_timeout() {
        let (_tx, rx) = mpsc::channel::<FlashEvent>();
        let mut watcher = ChannelWatcher::new(rx);

        let deadline = Instant::now() + Duration::from_millis(10);
        let event = watcher.next_event(deadline).unwrap();
        assert!(event.is_none());
    }

    #[test]
    fn channel_watcher_returns_none_on_disconnect() {
        let (tx, rx) = mpsc::channel::<FlashEvent>();
        drop(tx);

        let mut watcher = ChannelWatcher::new(rx);
        let deadline = Instant::now() + Duration::from_secs(1);
        let event = watcher.next_event(deadline).unwrap();
        assert!(event.is_none());
    }

    #[test]
    fn channel_watcher_returns_none_on_expired_deadline() {
        let (_tx, rx) = mpsc::channel::<FlashEvent>();
        let mut watcher = ChannelWatcher::new(rx);

        let deadline = Instant::now(); // already expired
        let event = watcher.next_event(deadline).unwrap();
        assert!(event.is_none());
    }

    #[test]
    fn channel_watcher_delivers_multiple_events_in_order() {
        let (tx, rx) = mpsc::channel();
        let mut watcher = ChannelWatcher::new(rx);

        for i in 0..3 {
            let discovery = FlashDiscovery {
                name: format!("dev{i}"),
                dev_path: None,
                mountpoints: vec![PathBuf::from(format!("/mnt/dev{i}"))],
                serial: Some(format!("SER{i}")),
                vendor: None,
                model: None,
                fs_type: None,
                removable: None,
                vendor_id: None,
                product_id: None,
            };
            tx.send(FlashEvent::Added(discovery)).unwrap();
        }
        drop(tx);

        let deadline = Instant::now() + Duration::from_secs(1);
        for i in 0..3 {
            let event = watcher.next_event(deadline).unwrap().unwrap();
            if let FlashEvent::Added(d) = event {
                assert_eq!(d.name, format!("dev{i}"));
            } else {
                panic!("expected Added event");
            }
        }
    }

    #[test]
    fn channel_watcher_handles_remove_events() {
        let (tx, rx) = mpsc::channel();
        let mut watcher = ChannelWatcher::new(rx);

        tx.send(FlashEvent::Removed(FlashId {
            serial: Some("SER1".into()),
            dev_path: Some("/dev/sda1".into()),
        }))
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        let event = watcher.next_event(deadline).unwrap();
        assert!(matches!(event, Some(FlashEvent::Removed(_))));
    }
}
