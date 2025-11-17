# zmk-layout-rs

Rust port of the ZMK layout tooling. This crate mirrors the Python implementation and gradually re-implements the tokenizer, parser, serializer, providers, and adapters described in `rust/PLAN.md`.

## Structure

```
zmk-layout-rs/
├── src/
│   ├── tokenizer/     # Logos-based lexer that preserves spans
│   ├── parser/        # AST construction + diagnostics
│   ├── serialization/ # Devicetree writer
│   ├── providers/     # Binding editors, behavior/combo helpers
│   └── adapters/      # Standard JSON bridge
├── tests/             # Integration tests that track PLAN fixtures
├── examples/          # Sample binaries demonstrating adapters
└── Cargo.toml
```

## Getting Started

```bash
cd rust/zmk-layout-rs
cargo test    # run the entire suite
```

Tests mirror the Python fixtures from `tests/fixtures/` to ensure feature parity during the port.

## Adapter Example CLI

The `examples/standard_cli.rs` program showcases how to convert between DTS and the standard JSON format via the adapter module.

```bash
cargo run --example standard_cli -- export \
  --dts tests/fixtures/ast_walker_complex.dts \
  --json /tmp/layout.json

cargo run --example standard_cli -- import \
  --json /tmp/layout.json \
  --template tests/fixtures/ast_walker_complex.dts \
  --output /tmp/layout_imported.dts
```

## Development Notes

- Follow `rust/PLAN.md` for the phased implementation order.
- Every turn must update `rust/CHANGELOG.md` with the change summary and next step.
- Prefer `cargo fmt` before submitting changes.
- External dependencies are limited to `logos`, `chumsky` (future), `thiserror`, `serde`, and `clap` (examples) unless justified by PLAN.
