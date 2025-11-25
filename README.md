# zmk-layout-rs

Rust port of the ZMK layout tooling stack. The crate parses Devicetree keymaps,
offers structured editing primitives, and bridges layouts to a stable JSON
format that can be consumed by GUI editors and other automation.

See `CLI_CODEBASE_REFACTOR_PLAN.md` for the refactor roadmap; the crate is written without `unsafe`.

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
  `DtsDocument`s (layers, combos, behavior metadata) with structured errors; modules are split into `layers`, `combos`, `behaviors`, and shared `format`/`util` helpers.
- **CLI + shared IO** – `cli` hosts clap definitions + per-command handlers and relies on `io` helpers for layout/task loading, serialization, and diff rendering.
- **Standard adapter** – `adapters::standard` converts between Devicetree and a
  JSON schema (`layers`, `combos`, `behaviors`, `metadata`) for use by other
  projects, with a unified `adapters::pipeline` that loads JSON/DTS (paths or text) and optionally captures template metadata.
- **Firmware builds** – `build` provides manifest parsing, toolchain orchestration, cache/workspace management, and layout staging (including adapter pipelines) for the `zmk-layout firmware` commands.
- **Flash fakes** – set `ZMK_FLASH_FAKE_BACKEND=1` plus `ZMK_FLASH_FAKE_MOUNTPOINT`, `ZMK_FLASH_FAKE_NAME`, `ZMK_FLASH_FAKE_SERIAL`, `ZMK_FLASH_FAKE_VENDOR`, `ZMK_FLASH_FAKE_MODEL`, and `ZMK_FLASH_FAKE_FSTYPE` to run firmware `devices`/`flash` commands without hardware.

```
src
├── adapters/              # Standard/MoErgo adapters + unified pipeline/bundles
│   ├── standard/          # JSON import/export/helpers + template renderers
│   └── pipeline.rs        # JSON/DTS loader with optional template capture
├── ast/                   # AST definitions + walkers
├── bindings/              # Binding parser & normalization rules
├── build/                 # Firmware builder/toolchains/workspaces/progress
├── cli/                   # clap app + per-command handlers + shared context
├── dts/                   # High-level DtsDocument wrapper
├── flash/                 # Flash core + platform backends (feature-gated)
├── io/                    # Shared layout/task IO + diff helpers
├── layout_engine/         # Task execution plumbing over KeymapDocument
├── macro_support/         # Macro registry & expansion
├── parser/                # Devicetree parser
├── providers/             # Keymap/behavior/combo helpers
├── serialization/         # Serializer back to DTS text
└── tasks/                 # Task config/targets/lua_engine + executor

See `docs/codebase_overview.md` for a contributor-oriented summary of module boundaries and feature flags.
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

### Adapter pipeline (JSON/DTS sources)

Load layouts from JSON or DTS (paths or text):

```rust
use zmk_layout_rs::adapters::AdapterPipeline;

fn load_any_layout() -> Result<(), Box<dyn std::error::Error>> {
    // From JSON path
    let json_layout = AdapterPipeline::from_json_path("layout.json").load()?;

    // From DTS
    let rendered = std::fs::read_to_string("rendered.dts")?;
    let layout = AdapterPipeline::from_dts_text(rendered).load()?;

    println!("layers: {}", layout.layers.len());
    Ok(())
}
```

### Keyboard profiles

Use the TOML keyboard profiles to hydrate layouts and resolve template metadata:

```rust
use std::path::PathBuf;
use zmk_layout_rs::{
    adapters::import_standard_file_for_profile,
    profiles::KeyboardProfileDoc,
};

fn hydrate_profile_layout() -> Result<(), Box<dyn std::error::Error>> {
    let profile = KeyboardProfileDoc::from_file("profiles/keyboards/glove80.toml")?;
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let hydrated =
        import_standard_file_for_profile("layout.json", &profile, &repo_root)?;
    hydrated.write_to_file("glove80.generated.dts")?;
    Ok(())
}
```

Validate profiles directly from the CLI:

```bash
zmk-layout profiles check profiles/keyboards/glove80.toml
zmk-layout profiles check --all
```

The `--all` flag scans every `*.toml` inside `profiles/keyboards/` (override with
`--profiles-dir DIR`).

### Customization Tasks & CLI

The crate now ships with a dedicated task runner so you can replay layout tweaks whenever the base
template changes. Task files follow the schema described in `docs/customization_tasks.md`. Use the
`zmk-layout` binary to apply, validate, or diff your changes:

```bash
# Apply tasks and write the result
zmk-layout keymap apply --tasks layout_tasks.toml --base-layout config/keymap.dts --output config/keymap.generated.dts

# Dry-run to inspect conflicts without touching the file
zmk-layout keymap validate --tasks layout_tasks.toml --base-layout config/keymap.dts

# Review a unified diff in the terminal
zmk-layout keymap diff --tasks layout_tasks.toml --base-layout config/keymap.dts
```

Helpful flags:

- `--conflicts override|skip|prompt|script` overrides the task file's default conflict policy for a single run.
- `--base-template NAME` / `--base-version VERSION` document which upstream template/version you expected and warn when the task file disagrees.
- `--combo-conditions` prints a post-run summary of combo tasks that declare `conditions = ["..."]` for easier review.

See the docs for conflict policies, `target` naming guidance, and troubleshooting tips.
The same document covers the Lua scripting hooks that power `script` tasks and conflict handlers.

#### Preprocessing zmk-helpers (optional)

Build with `--features ancpp-preprocessor` to enable C-preprocessing before parsing DTS files (helps when keymaps use zmk-helpers macros). When the feature is on, CLI flags appear on DTS-consuming commands:

- `--preprocess` opt-in flag; add `--cpp-include DIR` for zmk-helpers and your config dir, `--cpp-system-include DIR` for Zephyr/ZMK headers, and `--cpp-define NAME[=VALUE]` for things like `HOST_OS=2`.
- Tasks/Script: `zmk-layout keymap apply/validate/diff` and `zmk-layout keymap lua` accept the flags and preprocess `--base-layout` / `--layout` first.
- Keymap export: `zmk-layout keymap convert --preprocess ... --input config/keymap.dts --output layout.json --from dts --to json` expands macros before exporting JSON. You can supply `--profile <name>` to auto-apply profile-specific extraction (e.g., MoErgo regex capture) instead of plain DTS parsing.
- Keymap render: `zmk-layout keymap convert --input layout.json --output templates/keymap.dtsi --from json --to dts --template templates/keymap.dtsi.j2` writes DTS from JSON. You can pass `--profile <name>` instead of `--template` to resolve the template from the keyboard profile.
- MoErgo JSON: `--from moergo-json` / `--to moergo-json` round-trip Keymap-Editor exports like the samples under `examples/samples/*.json`.
- Firmware build: `zmk-layout firmware build --preprocess ... --layout-dts config/keymap.dts ...` preprocesses the DTS before feeding the build pipeline.

The ancpp crate is MPL-2.0 with additional terms; keep it feature-gated if your project requires MIT/Apache-only dependencies.

### Lua Scripts

For one-off automation or debugging, run Lua scripts directly without creating a full task file:

```bash
zmk-layout keymap lua \
  --script tasks/swap_layer_names.lua \
  --layout config/keymap.dts \
  --output config/keymap.generated.dts

# Preview without writing a file
zmk-layout keymap lua --script scratch/update_layers.lua --layout config/keymap.dts --diff
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

Keyboard profiles are single TOML documents that collect a keyboard’s metadata,
hardware facts, firmware catalog, and layout template hints. Firmware manifests
point at these profiles to understand what they are building. See
`docs/keyboard_profiles.md` for the full specification and authoring guidance.

### Running a firmware build

```bash
zmk-layout firmware build \
  --manifest profiles/firmwares/glove80.toml \
  --keyboard glove80 \
  --toolchain zmk \
  --target left \
  --layout-dts config/keymap.generated.dts \
  --output dist/glove80-left
```

Key options: supply exactly one layout input (`--layout-json`, `--layout-dts`, or `--keymap` plus an optional `--kconfig`),
repeat `--target` to limit which manifest targets build, use `--env KEY=VALUE` for ad-hoc environment overrides, add
`-D/--kconfig-def NAME=VALUE` to append Kconfig options (also forwarded as west `-D` args), and enable
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

The image name `moergo-zmk-config-docker:latest` is referenced in `profiles/firmwares/glove80.toml`
and is used by the build system to compile firmware for MoErgo keyboards.

## Development

- Run the full test suite: `cargo test`
- Lint/format (optional but recommended): `cargo fmt && cargo clippy`
- New contributors can explore behavior in `tests/` and the fixtures under
  `tests/fixtures`.

Issues, ideas, and PRs are welcome—please include tests alongside functional
changes so the parser/serializer guarantees stay solid.
