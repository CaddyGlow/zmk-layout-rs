# Codebase Overview

This document summarizes the main modules and how they fit together for contributors working on the refactored CLI/codebase.

- `src/cli/`
  - `app.rs` defines the clap surface.
  - `commands/*` hold thin handlers that wire arguments to library calls (tasks, script, layer, firmware, profiles).
  - `context.rs` and `error.rs` centralize shared prep/printing logic; `preprocess.rs` is feature-gated behind `ancpp-preprocessor`.
- `src/io/`
  - Small helpers for reading/writing text, loading layouts/task files (with optional preprocessing), serializing keymaps, and rendering unified diffs.
  - Consumed by the CLI and tests to keep fs/error handling consistent.
- `src/tasks/`
  - `config.rs` parses/validates TOML task files and materializes actions/targets.
  - `targets.rs` normalizes target paths and checks for overlaps.
  - `lua_engine.rs` hosts the embedded Lua runtime used by `script` tasks and conflict hooks.
  - `engine.rs` executes tasks against `KeymapDocument`, reusing the above helpers; `mod.rs` re-exports the public surface.
- `src/flash/`
  - `core.rs` defines shared types, the `FlashBackend` trait, fake/env backend hooks, and copy/board-id validation logic.
  - Platform-specific backends live in `linux.rs`, `macos.rs`, `windows.rs` with a `stub.rs` fallback; features `flash-*` gate the platform code.
- `src/providers/`
  - Split into `layers.rs`, `combos.rs`, `behaviors.rs`, `format.rs`, and `util.rs` to keep binding normalization and document mutations focused.
- `src/adapters/`
  - `standard/` exposes the JSON <-> Devicetree adapter, template renderers, and property-order helpers.
  - `pipeline.rs` offers a unified loader for JSON/DTS inputs with optional template capture.
- `src/build/`
  - Builder/toolchain/workspace layers for firmware builds plus layout staging in `layout.rs`.
  - `BuildRequest` supports multiple layout sources, including adapter pipelines and DTS documents.

Feature flags:
- `flash-linux`, `flash-macos`, `flash-windows`, `flash-fake-backend` (default-enabled) guard platform flash backends and the fake/env test harness.
- `ancpp-preprocessor` enables preprocessing for layout inputs before parsing.
