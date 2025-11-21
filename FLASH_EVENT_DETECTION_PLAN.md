# Flash Event-Driven Detection Plan

Goal: add optional event-driven hotplug detection for firmware flashing, selectable via CLI, while preserving the current polling path as an explicit alternative.

## High-Level Approach
- Add a small platform watcher abstraction that emits events on device attach/mount changes.
- Keep `discover_devices` and `wait_for_device` semantics stable; selection between event mode and polling is done via a CLI flag (no automatic fallback).
- Hide event support behind per-platform feature flags (e.g., `flash-events-linux`, `flash-events-macos`, `flash-events-windows`) so builds stay lean and failures stay localized.

## API / Symbol Changes
- New types in `src/flash/mod.rs` (feature-gated):
  - `struct FlashId { serial: Option<String>, dev_path: Option<String> }` captured on add.
  - `enum FlashEvent { Added(FlashDiscovery), Removed(FlashId) }`
  - `trait FlashWatcher: Send` with `fn next_event(&mut self, until: Instant) -> io::Result<Option<FlashEvent>>`
  - `fn platform::start_watcher(query: Option<&str>) -> Result<Box<dyn FlashWatcher>, FlashError>`
- Adjusted flow:
  - `wait_for_device` uses watcher mode when the CLI flag selects it; otherwise it uses the existing polling loop. No automatic fallback from events to polling.
  - `discover_devices` stays untouched, still exposed for ad-hoc listing and polling mode.
- CLI remains unchanged; only behavior improves when the feature is on.

## Platform Implementations (event path)
- **Linux** (`flash-events-linux`): use `udev` Monitor (`udev` crate) for `add`/`change` on block devices; resolve to `FlashDiscovery` with one immediate `lsblk` pass per event for mountpoints.
- **macOS** (`flash-events-macos`): prefer `diskutil monitor -plist` streaming parser (no new native bindings) to get attach/mount events; map to `FlashDiscovery`.
- **Windows** (`flash-events-windows`): use `Win32_VolumeChangeEvent` via WMI (`windows` crate) or a small PowerShell `Register-WmiEvent` child process that streams JSON; parse to `FlashDiscovery`.
- Each watcher feeds events into a channel; `next_event` pops or waits with a timeout boundary.

## Fallback and Rollback
- If the watcher cannot start or returns errors while the CLI flag selects event mode, surface the failure (e.g., `FlashError::ProbeFailed`) and instruct users to retry with polling mode.
- Feature flags are per-platform; disabling them (or not enabling the CLI flag) keeps legacy polling behavior.
- No public API changes to callers; only internal branching and the runtime mode switch.

## Incremental Implementation Steps
1. **Foundation**: Add per-platform feature gates, event types, `FlashWatcher` trait, and the watcher-backed path in `wait_for_device` behind the CLI flag.
2. **Linux watcher**: Implement udev-based watcher; wire into `platform::start_watcher`.
3. **macOS watcher**: Implement `diskutil monitor` parser; gate it under the same feature.
4. **Windows watcher**: Implement WMI/event-driven watcher; integrate.
5. **Resilience**: Ensure duplicate-serial filtering still applies, preserve `mount_timeout` semantics, and fail clearly when event mode is selected but unavailable.
6. **Testing**: Unit-test the parsers (udev/diskutil/PowerShell outputs) and add an integration-style test with synthetic events where possible; manual validation per OS.
7. **Release switch**: Keep the feature off by default initially; document the flag and how to revert to polling.

## Open Questions
- Which async runtime to align with (tokio vs. small blocking threads)? Current CLI is sync; we can hide async behind threads + channels, then revisit async/await later.
- Should the CLI flag default to polling (opt-in to events) or remember the last choice?
