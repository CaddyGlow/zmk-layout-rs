# zmk-layout-rs

Rust tooling for ZMK keyboard layouts. Parses Devicetree keymaps, provides
structured editing, and bridges layouts to a stable JSON format for GUI editors
and automation.

## Highlights

- **Parser** -- `logos`-powered lexer and recursive descent parser producing a
  strongly typed AST from Devicetree sources.
- **Serializer** -- deterministic formatting that preserves original style.
- **Macro support** -- evaluates `#define`s, conditionals, and template blocks.
- **Binding editing** -- understands ZMK behaviors (tap/hold, mod chains) and
  normalizes them for round-tripping.
- **Providers** -- ergonomic mutation APIs for layers, combos, and behaviors
  (see `docs/codebase_overview.md`).
- **Standard adapter** -- JSON import/export for layers, combos, behaviors, and
  metadata via `adapters::standard` and the unified `adapters::pipeline`.
- **Task engine** -- declarative TOML tasks and Lua scripts for repeatable
  keymap modifications (see `docs/customization_tasks.md`).
- **Firmware builds** -- manifest-driven Docker builds for MoErgo and ZMK
  toolchains (see `docs/firmware_building.md`).
- **Firmware flashing** -- USB mass storage flashing with device detection
  (see `docs/firmware_flashing.md`).
- **Keyboard profiles** -- TOML-based keyboard metadata embedded in the binary
  (see `docs/keyboard_profiles.md`).

## Getting Started

Requirements: Rust toolchain with the 2024 edition (`rustup default nightly`
until the edition stabilizes).

```bash
cargo add zmk-layout-rs --path rust/zmk-layout-rs
```

## Usage

### Parse and edit keymaps

```rust
use zmk_layout_rs::{
    dts::DtsDocument,
    providers::{KeymapProvider, ProviderError},
};

fn add_escape_binding() -> Result<(), ProviderError> {
    let document = DtsDocument::parse_file("config/keymap.dts")?;
    let mut provider = KeymapProvider::new(document);

    let layers = provider.layer_names();
    println!("layers: {layers:?}");

    provider.set_binding("base", 0, "&kp ESC")?;

    let updated = provider.into_document();
    updated.write_to_file("config/keymap.generated.dts")?;
    Ok(())
}
```

### JSON round-trip

```rust
use zmk_layout_rs::{
    adapters::{export_standard_file, import_standard_file_with_template},
    dts::DtsDocument,
};

fn round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let document = DtsDocument::parse_file("keymap.dts")?;
    export_standard_file(&document, "layout.json")?;

    let hydrated = import_standard_file_with_template("layout.json", "template.dtsi")?;
    hydrated.write_to_file("keymap.from_json.dts")?;
    Ok(())
}
```

JSON shape (all fields optional unless noted):

```json
{
  "title": "Corne-ish Zen",
  "metadata": { "keyboard": "cradio" },
  "layers": [{ "name": "base", "bindings": ["&kp Q", "&kp W"] }],
  "combos": [{ "name": "copy", "key_positions": [0, 1], "bindings": ["&kp C"] }],
  "behaviors": [{ "name": "caps_word", "bindings": ["&caps_word"] }]
}
```

### Adapter pipeline

Load layouts from JSON or DTS (paths or inline text):

```rust
use zmk_layout_rs::adapters::AdapterPipeline;

fn load_any_layout() -> Result<(), Box<dyn std::error::Error>> {
    let json_layout = AdapterPipeline::from_json_path("layout.json").load()?;
    let rendered = std::fs::read_to_string("rendered.dts")?;
    let layout = AdapterPipeline::from_dts_text(rendered).load()?;
    println!("layers: {}", layout.layers.len());
    Ok(())
}
```

### Keyboard profiles

```rust
use std::path::PathBuf;
use zmk_layout_rs::{
    adapters::import_standard_file_for_profile,
    profiles::KeyboardProfileDoc,
};

fn hydrate_profile_layout() -> Result<(), Box<dyn std::error::Error>> {
    // Load by name (checks filesystem, falls back to embedded)
    let profile = KeyboardProfileDoc::load("glove80")?;
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let hydrated = import_standard_file_for_profile("layout.json", &profile, &repo_root)?;
    hydrated.write_to_file("glove80.generated.dts")?;
    Ok(())
}
```

Validate profiles:

```bash
zmk-layout profiles check profiles/keyboards/glove80/profile.toml
zmk-layout profiles check --all
```

### Prelude

```rust
use zmk_layout_rs::prelude::*;

fn list_layers() -> Result<(), Box<dyn std::error::Error>> {
    let doc = DtsDocument::parse_file("config/keymap.dts")?;
    let provider = KeymapProvider::new(doc);
    println!("layers: {:?}", provider.layer_names());
    Ok(())
}
```

## CLI Reference

### Keymap commands

```bash
# Apply tasks and write the result
zmk-layout keymap apply --tasks layout_tasks.toml --base-layout config/keymap.dts \
  --output config/keymap.generated.dts

# Dry-run validation
zmk-layout keymap validate --tasks layout_tasks.toml --base-layout config/keymap.dts

# Preview as unified diff
zmk-layout keymap diff --tasks layout_tasks.toml --base-layout config/keymap.dts

# Run a Lua script
zmk-layout keymap lua --script tasks/swap_layer_names.lua \
  --layout config/keymap.dts --output config/keymap.generated.dts

# Convert between formats
zmk-layout keymap convert --input keymap.dts --output layout.json --from dts --to json
zmk-layout keymap convert --input layout.json --output keymap.dtsi --from json --to dts \
  --template templates/keymap.dtsi.j2
```

Useful flags: `--conflicts override|skip|prompt|script`, `--combo-conditions`,
`--preprocess` (requires `ancpp-preprocessor` feature), `--profile <name>`.

### Firmware commands

```bash
# Build firmware
zmk-layout firmware build \
  --manifest glove80 --keyboard glove80 --toolchain zmk \
  --target left --layout-dts config/keymap.generated.dts \
  --output dist/glove80-left

# Flash firmware
zmk-layout firmware flash --manifest glove80 --keyboard glove80 \
  --artifacts dist/

# List detected devices
zmk-layout firmware devices --manifest glove80 --keyboard glove80
```

Layout input: supply `--layout-json`, `--layout-dts`, or `--keymap` plus optional
`--kconfig`. Other flags: `--env KEY=VALUE`, `-D/--kconfig-def NAME=VALUE`,
`--disable-cache`, `--dry-run`.

### Lua scripts

```lua
layout:layer("base"):bind(2, "&kp TAB"):apply()
layout:combo("copy"):keys({28, 29}):binding("&kp C"):apply()
log("patched base layer")
```

See `docs/layer_api.md` for the full Lua API and `docs/customization_tasks.md`
for the task file format.

## Docker toolchain

Build the MoErgo toolchain image:

```bash
docker build -t moergo-zmk-config-docker:latest \
  -f ./toolchains/moergo/Dockerfile.debian ./toolchains/moergo/
```

The image name is referenced in `profiles/firmwares/glove80.toml`.

## Testing without hardware

Set environment variables to use the fake flash backend:

```bash
export ZMK_FLASH_FAKE_BACKEND=1
export ZMK_FLASH_FAKE_MOUNTPOINT=/tmp/fake-mount
export ZMK_FLASH_FAKE_NAME=FAKE_DEVICE
export ZMK_FLASH_FAKE_SERIAL=GLV80-FAKE
export ZMK_FLASH_FAKE_VENDOR=DemoVendor
export ZMK_FLASH_FAKE_MODEL=DemoModel
export ZMK_FLASH_FAKE_FSTYPE=vfat
```

## Development

```bash
cargo test
cargo fmt && cargo clippy
```

Tests and fixtures live under `tests/` and `tests/fixtures/`.
