# zmk-layout-rs

Rust port of the ZMK layout tooling stack. The crate parses Devicetree keymaps,
offers structured editing primitives, and bridges layouts to a stable JSON
format that can be consumed by GUI editors and other automation.

The implementation follows the staged roadmap in `rust/PLAN.md` and is written
without `unsafe`.

## Highlights

- **Tokenizer & parser** – `logos`-powered lexer (`tokenizer`) and recursive
  descent parser (`parser`) that lift Devicetree syntax into a strongly typed
  AST (`ast`).
- **Serializer** – deterministic formatting via `serialization`, so documents
  you touch stay close to the original style.
- **Macro & template helpers** – `macro_support` evaluates `#define`s,
  conditionals, and template blocks so downstream tooling can treat expansions
  as plain text.
- **Binding-aware editing** – `bindings` understands ZMK behaviors (tap/hold,
  mod chains, etc.) and normalizes them for consistent round‑tripping.
- **Provider APIs** – `providers` exposes ergonomic methods for mutating
  `DtsDocument`s (layers, combos, behavior metadata) with structured errors.
- **Standard adapter** – `adapters::standard` converts between Devicetree and a
  JSON schema (`layers`, `combos`, `behaviors`, `metadata`) for use by other
  projects.

```
src
├── adapters/standard.rs   # JSON import/export helpers
├── ast/                   # AST definitions + walkers
├── bindings/              # Binding parser & normalization rules
├── dts/                   # High level DtsDocument wrapper
├── macro_support/         # Macro registry & expansion
├── parser/                # Devicetree parser
├── providers/             # Keymap/behavior/combo helpers
├── serialization/         # Serializer back to DTS text
└── tokenizer/             # Logos-based tokenizer
```

## Getting Started

Requirements:

- Rust toolchain with the 2024 edition enabled (`rustup default nightly` until
  the edition stabilizes).

Add the crate to your project (local path or git until published):

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

### Convert to and from the standard JSON format

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

The emitted JSON has this shape (all fields optional unless noted):

```json
{
  "title": "Corne-ish Zen",
  "metadata": { "keyboard": "cradio" },
  "layers": [{ "name": "base", "bindings": ["&kp Q", "&kp W"] }],
  "combos": [{ "name": "copy", "key_positions": [0, 1], "bindings": ["&kp C"] }],
  "behaviors": [{ "name": "caps_word", "bindings": ["&caps_word"] }]
}
```

### Example CLI

`examples/standard_cli.rs` demonstrates a minimal converter:

```bash
cargo run --example standard_cli -- export \
  --dts tests/fixtures/keymap.dts \
  --json layout.json

cargo run --example standard_cli -- import \
  --json layout.json \
  --template tests/fixtures/keymap_template.dts \
  --output keymap.new.dts

cargo run --example standard_cli -- import \
  --json layout.json \
  --template examples/moergo_glove80.j2 \
  --output keymap.moergo.dts
```

### Customization Tasks & CLI

The crate now ships with a dedicated task runner so you can replay layout tweaks whenever the base
template changes. Task files follow the schema described in `docs/customization_tasks.md`. Use the
`zmk-layout` binary to apply, validate, or diff your changes:

```bash
# Apply tasks and write the result
zmk-layout apply --tasks layout_tasks.toml --base-layout config/keymap.dts --output config/keymap.generated.dts

# Dry-run to inspect conflicts without touching the file
zmk-layout validate --tasks layout_tasks.toml --base-layout config/keymap.dts

# Review a unified diff in the terminal
zmk-layout diff --tasks layout_tasks.toml --base-layout config/keymap.dts
```

Helpful flags:

- `--conflicts override|skip|prompt|script` overrides the task file's default conflict policy for a single run.
- `--base-template NAME` / `--base-version VERSION` document which upstream template/version you expected and warn when the task file disagrees.
- `--combo-conditions` prints a post-run summary of combo tasks that declare `conditions = ["..."]` for easier review.

See the docs for conflict policies, `target` naming guidance, and troubleshooting tips.
The same document covers the Rhai scripting hooks that power `script` tasks and conflict handlers.

### Standalone Rhai Scripts

For one-off automation or debugging, run Rhai scripts directly without creating a full task file:

```bash
zmk-layout script \
  --script tasks/swap_layer_names.rhai \
  --layout config/keymap.dts \
  --output config/keymap.generated.dts

# Preview without writing a file
zmk-layout script --script scratch/update_layers.rhai --layout config/keymap.dts --diff
```

Scripts receive the same helper API as `script` tasks (`set_binding`, `set_layer`, `upsert_combo`, etc.) and can emit notes
with `log("...")`. Pass `--diff` to print a unified diff instead of writing files, or omit `--output` entirely to stream the
updated DTS to stdout. See `docs/customization_tasks.md` for the full scripting surface.

### Template-based generation

If your DTS template uses `{{ … }}` placeholders (similar to the Python implementation),
the adapter can expand the template directly. Provide metadata extras in the JSON
(`includes`, `custom_devicetree`, `input_processors`, etc.) and call
`import_standard_file_with_template`. Known placeholders such as `{{combos}}`,
`{{macros}}`, `{{behaviors}}`, `{{rendered_layers}}`, and `{{layer_names_defines}}`
are populated from the layout data. See `examples/moergo_glove80.j2` for a
full-featured template mirroring the Python generator output.

## Firmware Build Toolchain

The project includes Docker-based toolchains for building ZMK firmware. The MoErgo toolchain
(formerly known as glove80-zmk-config) is located in `toolchains/moergo/`.

## Keyboard Profiles

Keyboard profiles describe a keyboard’s hardware spec, firmware catalog, available
behaviors/combos, and layout templates in a structured YAML format. They live under
directories such as `example_profile/` and are referenced by firmware manifests
(`firmware_profiles/*.toml`). See `docs/keyboard_profiles.md` for the spec and
guidance on creating new profiles.

### Running a firmware build

```bash
zmk-layout firmware build \
  --manifest firmware_profiles/glove80.toml \
  --keyboard glove80 \
  --toolchain zmk \
  --target left \
  --layout-dts config/keymap.generated.dts \
  --output dist/glove80-left
```

Key options: supply exactly one layout input (`--layout-json`, `--layout-dts`, or `--keymap` plus an optional `--kconfig`),
repeat `--target` to limit which manifest targets build, use `--env KEY=VALUE` for ad-hoc environment overrides, and add
`--disable-cache` or `--dry-run` when you want clean workspaces or a printed request without touching Docker. For manifest
schema details, caching policies, and artifact/log expectations see [`docs/firmware_building.md`](docs/firmware_building.md).

Every `zmk-layout firmware build`
invocation emits:

- `build-<keyboard>-<toolchain>.log` with the combined Docker output.
- `build-info-<keyboard>-<toolchain>.json` summarizing metadata, targets, and artifacts.

### Building the Docker Image

Build the MoErgo toolchain image using one of the provided Dockerfiles:

```bash
# Recommended: Debian-based image (smaller, faster build)
docker build -t moergo-zmk-config-docker:latest -f ./toolchains/moergo/Dockerfile.debian ./toolchains/moergo/

# Alternative: Pure Nix-based image
docker build -t moergo-zmk-config-nix:latest -f ./toolchains/moergo/Dockerfile.nix ./toolchains/moergo/

# Alternative: Alpine-based Nix image
docker build -t moergo-zmk-config-docker:latest -f ./toolchains/moergo/Dockerfile ./toolchains/moergo/
```

The image name `moergo-zmk-config-docker:latest` is referenced in `firmware_profiles/glove80.toml`
and is used by the build system to compile firmware for MoErgo keyboards.

## Development

- Run the full test suite: `cargo test`
- Lint/format (optional but recommended): `cargo fmt && cargo clippy`
- New contributors can explore behavior in `tests/` and the fixtures under
  `tests/fixtures`.

Issues, ideas, and PRs are welcome—please include tests alongside functional
changes so the parser/serializer guarantees stay solid.
