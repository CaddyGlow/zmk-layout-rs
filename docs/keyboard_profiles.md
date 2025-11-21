# Keyboard Profile Specification (TOML)

Keyboard profiles are single TOML documents that capture everything the firmware
builder and layout tools need to know about a keyboard. This file replaces the
earlier ad-hoc YAML folders so we can describe keyboards with one canonical,
versioned document.

## Design Goals

- **Single source of truth** – manifests and CLIs reference one file instead of
  duplicating hardware or firmware data.
- **Human friendly TOML** – tables and arrays are enough for lists such as
  controllers, flash methods, or combo definitions; no YAML/JSON required.
- **Small required surface** – metadata, hardware, firmware, and layout tables
  are mandatory; everything else can be omitted when a keyboard does not need
  it.

## Top-Level Structure

Every profile must define at least these keys:

```toml
keyboard = "glove80"  # slug referenced by CLI/manifest
version = 1           # schema version for this profile

[metadata]            # required
[hardware]            # required
[firmware]            # required
[layout]              # required

# Optional sections
[[behaviors]]
[[combos]]
[[macros]]
```

If a keyboard does not use combos or macros, simply omit those arrays.

## Section Reference

### `metadata`

```toml
[metadata]
name = "MoErgo Glove80"
vendor = "MoErgo"
description = "Split ergonomic column-staggered keyboard"
homepage = "https://www.moergo.com"
tags = ["split", "wireless"]
```

Fields:
- `name` *(string, required)*
- `vendor` *(string, required)*
- `description`, `homepage`, `tags` *(optional)*

### `hardware`

```toml
[hardware]
key_count = 80
is_split = true
controllers = ["nrf52840"]

[[hardware.boards]]
id = "glove80_lh"
role = "left"

[[hardware.boards]]
id = "glove80_rh"
role = "right"

[[hardware.flash]]
method = "usb"
device_query = "serial~=GLV80-.* and removable=true"
mount_timeout = 120
copy_timeout = 60
sync_after_copy = true

[hardware.build_defaults.cmake]
CONFIG_ZMK_SPLIT_ROLE = "\"left\""

[hardware.build_defaults.env]
ARTIFACT_NAME = "glove80_left"
```

Required fields:
- `key_count` *(u32)*
- `is_split` *(bool)*

Optional:
- `controllers` *(array of strings)*
- `boards` *(array of tables with `id` and optional `role`/`variant`)*
- `flash` *(array; each table may define `method`, probe strings, timeouts, etc.)*
- `build_defaults` *(table; nested `cmake`, `env`, or `kconfig` maps that most
  builds should inherit)*

### `firmware`

```toml
[firmware]
default = "stable"

[firmware.versions.stable]
repository = "moergo-sc/zmk"
branch = "v25.05"
channel = "stable"
notes = "Vendor recommended release"

[firmware.versions.beta]
repository = "moergo-sc/zmk"
branch = "v25.08-beta.1"
channel = "beta"

[firmware.versions.beta.kconfig]
CONFIG_ZMK_RGB_UNDERGLOW = true
```

Required keys:
- `default` – string referencing one entry under `firmware.versions`.
- `versions.<id>` – each table supports:
  - `repository` *(string, required, `owner/repo` or full URL)*
  - `branch` *(string, required)*
  - `channel`, `notes` *(optional)*
  - Nested tables (`kconfig`, `env`, `combos`, `macros`) for per-firmware tweaks.

### `layout`

```toml
[layout]
template = "templates/keymap.dtsi.j2"

[layout.formatting]
key_gap = "  "
base_indent = ""

[[layout.formatting.rows]]
keys = [0, 1, 2, 3, 4, 5, -1, 40, 41, 42, 43, 44]

[layout.keymap]
header_includes = ["dt-bindings/zmk/bt.h"]
```

Required:
- `template` *(string)* – path to the DTS template used for generation.
- At least one entry under `layout.formatting.rows`. Represent each row as a
  table so we can extend it later (e.g., `[[layout.formatting.rows]] keys = [...]`).

Template paths can be absolute or relative to the workspace root you provide to
conversion helpers (e.g., `import_standard_file_for_profile`). This keeps the
profile portable across repos.

Optional:
- `formatting.key_gap` / `formatting.base_indent`
- `keymap.header_includes`
- `renderers.*` for alternative output templates

### Optional Arrays

Each optional array is a list of tables. Keep the structure lightweight so GUIs
or scripts can display and filter entries easily.

```toml
[[behaviors]]
id = "magic"
code = "&magic"
description = "Tap to show indicators; hold for layer shift"
expected_params = 0
origin = "vendor"

[[combos]]
name = "copy"
key_positions = [0, 1]
binding = "&kp C"
layers = ["base"]
timeout_ms = 30
```

The same pattern applies to `[[macros]]`.

## Minimal Example

```toml
keyboard = "planck"
version = 1

[metadata]
name = "Planck Rev6"
vendor = "OLKB"

[hardware]
key_count = 47
is_split = false

[firmware]
default = "stable"
[firmware.versions.stable]
repository = "zmkfirmware/zmk"
branch = "main"

[layout]
template = "templates/planck.dtsi.j2"
[[layout.formatting.rows]]
keys = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
```

This satisfies the required sections while staying concise. Tooling can enrich
it with defaults (flash method, board identifier, etc.), but every keyboard uses
the same TOML vocabulary so manifests and CLIs can resolve them consistently.

## Validation

Use the CLI to catch schema mistakes before committing:

```bash
zmk-layout profiles check profiles/keyboards/glove80.toml \
    tests/fixtures/profiles/bad_profile.toml
zmk-layout profiles check --all --profiles-dir tests/fixtures/profiles
```

Each file prints `[OK ]` on success or `[ERR]` with a descriptive validation
error if required sections are missing or malformed.
