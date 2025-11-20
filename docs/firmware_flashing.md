# Firmware Flashing Module (Glove80 UF2)

Design for a first firmware flashing module that matches the Glove80 "USB mass
storage" recovery flow. The goal is to copy a UF2 onto whichever half is in
bootloader mode, tolerate the device vanishing mid-copy, and ensure the left
image never lands on the right half (and vice-versa).

## Goals and Scope
- Support UF2-style flashing over USB mass storage (FAT32) with no vendor tools.
- Handle split boards where each half must receive the correct artifact, or a
  single artifact can be reused on both sides.
- Detect/mount the bootloader volume automatically and recover from the device
  rebooting before unmount completes.
- Provide both a CLI experience (`zmk-layout firmware flash`) and a reusable
  Rust API that accepts build reports or explicit artifact paths.
- Reuse keyboard profile metadata (`hardware.flash`, `hardware.boards`) so the
  workflow stays keyboard-aware.
- Out of scope: SWD/JTAG flashing, DFU, and custom bootloaders that do not
  expose a mass storage volume.

## UX and CLI Flow
- New command: `zmk-layout firmware flash --keyboard glove80 --artifacts dist`
  - Optional flags: `--side left|right|both`, `--firmware <path>`, `--build-info
    <build-info.json>`, `--device <mountpoint>`, `--no-sync`.
- Default flow when `--side` is absent for split boards:
  1. Prompt: "Put LEFT half in bootloader and plug it in."
  2. Wait for a matching device, mount if needed, copy the left artifact.
  3. Prompt: "Now repeat for RIGHT half." and flash the right artifact.
- If a single artifact was provided but the keyboard is split, reuse it for all
  requested sides (still validate side/device matching when possible).
- CLI connects to the existing progress reporter: checkpoints for "detect",
  "copy", "sync", "eject", and warnings for benign detach errors.

## Artifacts and Side Selection
- Source inputs (in priority order):
  1. `--firmware foo.uf2` (single file for every side)
  2. Explicit side flags `--left path --right path`
  3. `--build-info build-info-*.json` (read `artifacts.per_target` to locate
     files for targets named `left`/`right`; fall back to `artifacts.files`
     when only one artifact exists)
  4. Raw directory via `--artifacts DIR` (auto-pick `*left*.uf2`, `*right*.uf2`,
     or the only `.uf2` present)
- Internal representation:
  - `FlashSource::Single(PathBuf)`
  - `FlashSource::Split { left: PathBuf, right: PathBuf }`
  - `FlashTarget { side: FlashSide, board_id: Option<String> }` derived from
    `hardware.boards.role` and `hardware.boards.id`.

## Device Discovery and Mounting
- Reuse `hardware.flash[*]` entries:
  - `method = "usb"` (only supported method for v1)
  - `device_query` evaluated against OS device metadata.
  - `mount_timeout`, `copy_timeout`, `sync_after_copy` applied per attempt.
- Query evaluation:
  - Linux: `lsblk -J -O` for attributes (`serial`, `vendor`, `model`,
    `fstype`, `rm`, `size`, `mountpoints`, `label`).
  - macOS: `diskutil info -plist` for similar fields.
  - Windows: `Get-Volume` / `Get-CimInstance Win32_DiskDrive` for `SerialNumber`,
    `DriveType`, `FileSystem`, `DriveLetter`.
  - Supported predicates: `field=value`, `field~=regex`, combined with `and`.
    Example from the profile: `serial~=GLV80-.* and removable=true`.
- Mount resolution:
  - If the device is already mounted, reuse the first writable mountpoint.
  - Otherwise, mount to a temp directory under `$TMPDIR/zmk-layout-mounts`.
  - Surface a non-fatal warning when auto-mount fails and ask the user to mount
    manually with `--device /path/to/mount`.

## Copy/Flash Procedure
1. Locate the correct artifact for the requested side.
2. Wait for a matching device up to `mount_timeout` seconds.
3. Resolve mountpoint (auto-mount or user-supplied).
4. Optional side verification:
   - Read `INFO_UF2.TXT` (or `CURRENT.UF2` header) for a `Board-ID` string.
   - If it matches `hardware.boards.id` or contains `lh`/`rh` hints, ensure we
     are flashing the correct side; otherwise warn and prompt `--force`.
5. Stream-copy the artifact to the volume root using buffered I/O and track
   bytes written; respect `copy_timeout`.
6. `sync` if `sync_after_copy` is true. Treat `ENODEV`/`ENOENT` during sync or
   unmount as a warning (device likely rebooted early) when the full file was
   written; otherwise, fail the attempt.
7. Attempt best-effort unmount (ignore "not mounted" errors). Optionally wait
   for the device to disappear as a confirmation signal before proceeding.

## Glove80-Specific Notes
- Bootloader behavior: appears as a removable FAT32 volume; copying a UF2
  triggers an immediate reboot that can invalidate pending flush/unmount calls.
- Profile defaults: `device_query = "serial~=GLV80-.* and removable=true"`,
  `mount_timeout = 120`, `copy_timeout = 60`, `sync_after_copy = true`.
- Side detection heuristics:
  - Expect `INFO_UF2.TXT` to expose board hints like `glove80_lh` / `glove80_rh`
    (to be confirmed); fall back to prompting the user to confirm which side is
    connected when the ID is unknown.
  - If only one artifact is available, allow flashing both halves but still log
    the unresolved side identity.
- Happy path: copy completes; device vanishes; log success.
- Acceptable warning path: device vanishes during `sync`/`umount` after the full
  payload wrote; emit "Keyboard rebooted early; firmware likely flashing."

## Rust API Surface
- Module: `flash`
  - `FlashConfig` (derived from `HardwareFlash` + CLI overrides)
  - `FlashSource` (single/split artifacts)
  - `FlashDevice` (discovered device + mountpoint + board metadata)
  - `FlashStepResult` (progress + warnings)
  - `FirmwareFlasher::flash_target(target: FlashTarget, source: &FlashSource)`
    -> `FlashOutcome`
- Discovery helpers:
  - `probe_devices(config: &FlashConfig) -> Vec<FlashDevice>`
  - `read_board_id(mount: &Path) -> Option<String>`
- CLI wiring:
  - `zmk-layout firmware flash` builds a `FlashSource` from CLI/build-info,
    resolves `FlashTarget`s from the keyboard profile, and runs the flasher for
    each requested side in order.

## Symbol Reference (CLI + API)
- CLI entrypoint: `zmk-layout firmware flash`
  - Flags: `--keyboard`, `--side left|right|both`, `--firmware <uf2>`,
    `--left <uf2>`, `--right <uf2>`, `--build-info <json>`,
    `--artifacts <dir>`, `--device <mount>`, `--no-sync`, `--force`,
    `--mount-timeout <sec>`, `--copy-timeout <sec>`.
  - Reuses existing `--output-dir` concepts only for build; flashing writes
    nowhere else.
- New Rust types under `flash` (public):
  - `FlashSide` enum (`Left`, `Right`, `Both`)
  - `FlashTarget` (side + optional board id)
  - `FlashSource` (single or split artifact container)
  - `FlashConfig` (timeouts, queries, sync flag; seeded from `hardware.flash`)
  - `FlashDevice` (probe data + mountpoint + optional board id)
  - `FlashOutcome` / `FlashStepResult` (per-action telemetry + warnings)
  - `FirmwareFlasher` (orchestrator struct exposing `flash_target`)
- Profile fields consumed:
  - `hardware.flash[*].method` (must be `"usb"` for this path)
  - `hardware.flash[*].device_query`, `mount_timeout`, `copy_timeout`,
    `sync_after_copy`
  - `hardware.boards[*].id` / `role` (to map side -> expected board id)
  - Potential addition (TBD): `hardware.flash[*].role` if per-side overrides
    are needed.

## Error Handling and Telemetry
- Hard failures: no matching device within timeout, zero bytes copied, write
  errors before full length, conflicting side detection without `--force`.
- Warnings (non-fatal): device disappeared after full copy, unmount errors,
  missing `INFO_UF2.TXT`, multiple matching devices (prompt user to pick).
- Progress messages should always include: expected side, device identifier
  (serial/model), mount path, artifact name, and copy/sync durations.

## Open Questions / Follow-ups
- Confirm exact `Board-ID` or other token exposed by the Glove80 bootloader to
  disambiguate left vs right without user prompts.
- Decide whether to include an explicit `role` field under `hardware.flash`
  (e.g., `role = "left"`) to select per-side flash configs, or keep one shared
  entry plus `hardware.boards.role`.
- Evaluate Windows mounting behavior; we may need admin rights or a limited
  PowerShell fallback when auto-mounting.
- Should we retain a post-flash wait for the device to remount as a sanity
  check, or is "device disappeared after copy" sufficient?
