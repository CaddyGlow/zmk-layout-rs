# Changelog

## [Unreleased]

### Added
- Firmware build pipeline with Docker-based toolchains (MoErgo and ZMK Config/west)
- Firmware flashing via USB mass storage (`zmk-layout firmware flash`)
- Embedded keyboard/firmware profiles compiled into the binary via `rust-embed`
- TOML-based keyboard profile specification replacing legacy YAML folders
- `zmk-layout profiles check` command with `--all` and `--profiles-dir` flags
- Lua scripting engine for `script` tasks and `zmk-layout keymap lua`
- Declarative task system for repeatable keymap modifications
- Adapter pipeline for loading layouts from JSON or DTS sources
- C preprocessor support behind `ancpp-preprocessor` feature flag
- CLI commands: `keymap apply/validate/diff/lua/convert`, `firmware build/flash/devices`, `profiles check`
- Fake flash backend for testing without hardware (`ZMK_FLASH_FAKE_*` env vars)
- Python and Lua native binding modules (`crates/zmk-layout-python`, `crates/zmk-layout-lua-module`)
- Provider modules split into layers, combos, behaviors, format, and util
- Shared IO utilities for layout loading, serialization, and diff rendering
- Template-based DTS generation with `{{ ... }}` placeholder expansion

### Architecture
- CLI decomposed into `cli::app`, `cli::context`, and `cli::commands::*`
- Task engine split into `tasks::{config,targets,lua_engine,engine}`
- Flash module uses `FlashBackend` trait with platform-specific backends (feature-gated)
- Adapter stack provides JSON/DTS round-tripping through `adapters::standard` and `adapters::pipeline`
