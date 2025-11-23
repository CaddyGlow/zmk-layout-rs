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

pub use core::*;
