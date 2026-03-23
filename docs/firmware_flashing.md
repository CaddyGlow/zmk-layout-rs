# Firmware Flashing (UF2 over USB)

Copies UF2 firmware onto keyboards in USB mass storage bootloader mode. Supports
split boards where each half receives the correct artifact.

## Scope

- UF2-style flashing over USB mass storage (FAT32), no vendor tools.
- Split boards with per-side artifact validation.
- Automatic device detection and mounting.
- Out of scope: SWD/JTAG, DFU, custom bootloaders without mass storage.

## CLI

```bash
zmk-layout firmware flash \
  --manifest profiles/firmwares/glove80.toml \
  --keyboard glove80 \
  --artifacts dist/
```

### Flags

| Flag | Description |
|------|-------------|
| `--side left\|right\|both` | Which side(s) to flash (split boards default to both) |
| `--firmware <path>` | Single UF2 for all sides |
| `--left <path>` / `--right <path>` | Per-side UF2 files |
| `--build-info <json>` | Read artifact paths from a build-info file |
| `--artifacts <dir>` | Auto-pick UF2 files from a directory |
| `--device <mountpoint>` | Skip detection, use this mount directly |
| `--mount-timeout <sec>` | How long to wait for a device |
| `--copy-timeout <sec>` | Timeout for the copy operation |
| `--no-sync` | Skip sync after copy |
| `--detect poll\|events` | Device detection mode (default: poll) |

### Default flow (split boards)

1. Prompt: "Put LEFT half in bootloader and plug it in."
2. Detect device, mount if needed, copy left artifact.
3. Prompt: "Now repeat for RIGHT half."
4. Copy right artifact.

Non-split keyboards flash a single side.

### Artifact resolution

The CLI resolves artifacts in this priority order:

1. `--firmware foo.uf2` (single file for every side)
2. `--left path --right path` (explicit per-side)
3. `--build-info <json>` (reads `artifacts.per_target` for `left`/`right`)
4. `--artifacts <dir>` (auto-picks `*left*.uf2`, `*right*.uf2`, or the only `.uf2` present)

## Device Detection

Uses `hardware.flash` entries from the keyboard profile:

```toml
[[hardware.flash]]
method = "usb"
device_query = "serial~=GLV80-.* and removable=true"
mount_timeout = 120
copy_timeout = 60
sync_after_copy = true
```

Platform-specific discovery:
- **Linux**: `lsblk -J -O` (serial, vendor, model, fstype, mountpoints)
- **macOS**: `diskutil info -plist`
- **Windows**: `Get-Volume` / `Get-CimInstance Win32_DiskDrive`

Query predicates: `field=value`, `field~=regex`, combined with `and`.

If the device is already mounted, the first writable mountpoint is reused.
Otherwise it is mounted to `$TMPDIR/zmk-layout-mounts`.

## Copy procedure

1. Locate the correct artifact for the requested side.
2. Wait for a matching device up to `mount_timeout` seconds.
3. Optionally verify the side by reading `INFO_UF2.TXT` for a `Board-ID` string.
4. Stream-copy the artifact to the volume root.
5. `sync` if `sync_after_copy` is true.
6. Best-effort unmount.

Device vanishing after a complete copy is normal (the keyboard reboots into the
new firmware) and is reported as a warning, not an error.

## Rust API

Key types in the `flash` module:

- `FlashConfig` -- timeouts, queries, sync flag, detect mode (seeded from `hardware.flash`)
- `FlashSource` -- `Single(PathBuf)` or `Split { left, right }`
- `FlashTarget` -- side + optional board id + config
- `FlashDevice` -- discovered device + mountpoint + board metadata
- `FlashSideSelection` -- `Left`, `Right`, `Both`

Orchestration uses free functions: `flash_target()`, `discover_devices()`,
`build_flash_targets()`, `resolve_flash_source()`.

## Testing without hardware

See the fake backend environment variables in the main README.
