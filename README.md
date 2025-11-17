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

### Template-based generation

If your DTS template uses `{{ … }}` placeholders (similar to the Python implementation),
the adapter can expand the template directly. Provide metadata extras in the JSON
(`includes`, `custom_devicetree`, `input_processors`, etc.) and call
`import_standard_file_with_template`. Known placeholders such as `{{combos}}`,
`{{macros}}`, `{{behaviors}}`, `{{rendered_layers}}`, and `{{layer_names_defines}}`
are populated from the layout data. See `examples/moergo_glove80.j2` for a
full-featured template mirroring the Python generator output.

## Development

- Run the full test suite: `cargo test`
- Lint/format (optional but recommended): `cargo fmt && cargo clippy`
- New contributors can explore behavior in `tests/` and the fixtures under
  `tests/fixtures`.

Issues, ideas, and PRs are welcome—please include tests alongside functional
changes so the parser/serializer guarantees stay solid.
