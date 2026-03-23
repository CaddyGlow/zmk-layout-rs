# Codebase Overview

The project is a multi-crate workspace. The main crates:

| Crate | Purpose |
|-------|---------|
| `zmk-layout-core` | Core library: parser, providers, adapters, build, flash, tasks |
| `zmk-layout` | CLI binary (`zmk-layout`) with clap definitions and command handlers |
| `zmk-layout-lua` | Embedded Lua runtime for script tasks and conflict hooks |
| `zmk-layout-lua-module` | Standalone native Lua 5.4 module (separate build) |
| `zmk-layout-python` | PyO3-based Python bindings (separate build) |

## zmk-layout (CLI crate)

- `src/cli/app.rs` -- clap surface (commands, args, enums).
- `src/cli/commands/*` -- thin handlers that wire arguments to library calls.
- `src/cli/context.rs`, `src/cli/error.rs` -- shared prep/printing logic.
- `src/cli/preprocess.rs` -- feature-gated behind `ancpp-preprocessor`.

## zmk-layout-core (library crate)

- `src/io/` -- helpers for reading/writing text, loading layouts/tasks (with
  optional preprocessing), serializing keymaps, and rendering unified diffs.

- `src/tasks/`
  - `config.rs` -- parses/validates TOML task files and materializes actions/targets.
  - `targets.rs` -- normalizes target paths and checks for overlaps.
  - `script_backend.rs` -- `ScriptBackend` trait and `ScriptDecision` type for
    script task execution and conflict hooks.
  - `engine.rs` -- executes tasks against `KeymapDocument`.

- `src/flash/`
  - `core.rs` -- shared types, `FlashBackend` trait, fake/env backend hooks,
    copy/board-id validation, and free-function orchestration (`flash_target`,
    `discover_devices`).
  - Platform backends: `linux.rs`, `macos.rs`, `windows.rs`, `stub.rs` (feature-gated).

- `src/providers/`
  - `layers.rs` -- `KeymapProvider`/`KeymapDocument` plus layer editing.
  - `combos.rs` -- combo enumeration helpers and condition/comment handling.
  - `behaviors.rs` -- behavior enumeration with labels, timings, binding cells.
  - `format.rs` -- shared binding normalization/formatting via `BindingFormat`.
  - `util.rs` -- reusable AST utilities for locating/creating nodes/properties.

  Use `KeymapProvider`/`KeymapDocument` for mutations and the read-only
  combo/behavior providers for introspection.

- `src/adapters/`
  - `standard/` -- JSON/Devicetree adapter, template renderers, property-order helpers.
  - `pipeline.rs` -- unified loader for JSON/DTS inputs with optional template capture.

- `src/build/` -- builder, toolchain, workspace layers for firmware builds plus
  layout staging in `layout.rs`. `BuildRequest` supports multiple layout sources.

- `src/prelude.rs` -- re-exports common types/aliases for downstream consumers.

## zmk-layout-lua

- `src/lua_engine.rs` -- embedded Lua runtime (mlua) with the fluent `layout`
  API, builder objects, and `log()` global. Implements `ScriptBackend` for the
  task engine.

## Feature flags

| Flag | Purpose |
|------|---------|
| `flash-linux` | Linux flash backend |
| `flash-macos` | macOS flash backend |
| `flash-windows` | Windows flash backend |
| `flash-events-linux` | Event-driven device detection (Linux) |
| `flash-events-macos` | Event-driven device detection (macOS) |
| `flash-events-windows` | Event-driven device detection (Windows) |
| `flash-fake-backend` | Fake/env test harness (default-enabled) |
| `ancpp-preprocessor` | C preprocessing for layout inputs |
