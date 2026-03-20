//! USB mass-storage flashing helpers (UF2-style) for keyboards such as the Glove80.

mod core;
#[cfg(all(target_os = "linux", feature = "flash-linux"))]
mod linux;
#[cfg(all(target_os = "macos", feature = "flash-macos"))]
mod macos;
#[cfg(any(
    all(target_os = "linux", not(feature = "flash-linux")),
    all(target_os = "macos", not(feature = "flash-macos")),
    all(target_os = "windows", not(feature = "flash-windows")),
    not(any(target_os = "linux", target_os = "macos", target_os = "windows"))
))]
mod stub;
#[cfg(all(target_os = "windows", feature = "flash-windows"))]
mod windows;

// Polling platform alias
#[cfg(all(target_os = "linux", feature = "flash-linux"))]
use linux as platform;
#[cfg(all(target_os = "macos", feature = "flash-macos"))]
use macos as platform;
#[cfg(any(
    all(target_os = "linux", not(feature = "flash-linux")),
    all(target_os = "macos", not(feature = "flash-macos")),
    all(target_os = "windows", not(feature = "flash-windows")),
    not(any(target_os = "linux", target_os = "macos", target_os = "windows"))
))]
use stub as platform;
#[cfg(all(target_os = "windows", feature = "flash-windows"))]
use windows as platform;

// Event-driven watcher modules
mod watcher;

#[cfg(all(target_os = "linux", feature = "flash-events-linux"))]
mod linux_watcher;
#[cfg(all(target_os = "macos", feature = "flash-events-macos"))]
mod macos_watcher;
#[cfg(all(target_os = "windows", feature = "flash-events-windows"))]
mod windows_watcher;
mod watcher_stub;

// Watcher platform alias
#[cfg(all(target_os = "linux", feature = "flash-events-linux"))]
use linux_watcher as platform_watcher;
#[cfg(all(target_os = "macos", feature = "flash-events-macos"))]
use macos_watcher as platform_watcher;
#[cfg(all(target_os = "windows", feature = "flash-events-windows"))]
use windows_watcher as platform_watcher;
#[cfg(not(any(
    all(target_os = "linux", feature = "flash-events-linux"),
    all(target_os = "macos", feature = "flash-events-macos"),
    all(target_os = "windows", feature = "flash-events-windows"),
)))]
use watcher_stub as platform_watcher;

pub use core::*;
pub use watcher::{FlashEvent, FlashId, FlashWatcher};
