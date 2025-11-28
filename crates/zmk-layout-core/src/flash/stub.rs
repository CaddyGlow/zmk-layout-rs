use std::collections::HashSet;

use super::core::{FlashConfig, FlashDevice, FlashDiscovery, FlashError, FlashTarget};

pub(super) fn discover_devices(_config: &FlashConfig) -> Result<Vec<FlashDiscovery>, FlashError> {
    Err(FlashError::UnsupportedPlatform(
        "device discovery only supported on Linux, macOS, and Windows".into(),
    ))
}

pub(super) fn wait_for_device(
    _config: &FlashConfig,
    _target: Option<&FlashTarget>,
    _seen_serials: &HashSet<String>,
) -> Result<FlashDevice, FlashError> {
    Err(FlashError::UnsupportedPlatform(
        "automatic device discovery is not implemented for this platform".into(),
    ))
}
