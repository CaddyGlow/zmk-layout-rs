# Codebase Overview

Module boundaries and how they fit together for contributors.

## Core modules

- `src/cli/`
  - `app.rs` defines the clap surface.
  - `commands/*` hold thin handlers that wire arguments to library calls.
  - `context.rs` and `error.rs` centralize shared prep/printing logic.
  - `preprocess.rs` is feature-gated behind `ancpp-preprocessor`.

- `src/io/` -- helpers for reading/writing text, loading layouts/tasks (with
  optional preprocessing), serializing keymaps, and rendering unified diffs.
  Consumed by the CLI and tests.

- `src/tasks/`
  - `config.rs` parses/validates TOML task files and materializes actions/targets.
  - `targets.rs` normalizes target paths and checks for overlaps.
  - `lua_engine.rs` hosts the embedded Lua runtime for `script` tasks and conflict hooks.
  - `engine.rs` executes tasks against `KeymapDocument`.

- `src/flash/`
  - `core.rs` defines shared types, the `FlashBackend` trait, fake/env backend hooks, and copy/board-id validation.
  - Platform backends: `linux.rs`, `macos.rs`, `windows.rs`, `stub.rs` (feature-gated).

- `src/providers/`
  - `layers.rs` -- `KeymapProvider`/`KeymapDocument` plus layer editing (bindings, metadata, movement, defines).
  - `combos.rs` -- combo enumeration helpers and condition/comment handling.
  - `behaviors.rs` -- behavior enumeration with labels, timings, binding cells, and property capture.
  - `format.rs` -- shared binding normalization/formatting via `BindingFormat`.
  - `util.rs` -- reusable AST utilities for locating/creating nodes/properties.

  Use `KeymapProvider`/`KeymapDocument` for mutations and the read-only combo/behavior
  providers for introspection.

- `src/adapters/`
  - `standard/` exposes the JSON/Devicetree adapter, template renderers, and property-order helpers.
  - `pipeline.rs` offers a unified loader for JSON/DTS inputs with optional template capture.

- `src/build/` -- builder, toolchain, workspace layers for firmware builds plus
  layout staging in `layout.rs`. `BuildRequest` supports multiple layout sources
  including adapter pipelines and DTS documents.

- `src/prelude.rs` re-exports common types/aliases (`DtsKeymapDocument`,
  `KeymapProvider`, task engine types) for downstream consumers.

## Feature flags

| Flag | Purpose |
|------|---------|
| `flash-linux` | Linux flash backend |
| `flash-macos` | macOS flash backend |
| `flash-windows` | Windows flash backend |
| `flash-fake-backend` | Fake/env test harness (default-enabled) |
| `ancpp-preprocessor` | C preprocessing for layout inputs |
