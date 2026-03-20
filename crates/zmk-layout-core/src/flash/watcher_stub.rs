use super::core::FlashError;
use super::watcher::FlashWatcher;

#[allow(dead_code)]
pub(super) fn start_watcher(_query: Option<&str>) -> Result<Box<dyn FlashWatcher>, FlashError> {
    Err(FlashError::WatcherFailed(
        "event-driven detection is not available on this platform; use --detect poll".into(),
    ))
}
