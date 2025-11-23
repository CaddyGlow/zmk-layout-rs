# CLI & Codebase Refactoring Plan

## Goals
- Make the CLI easier to extend and reason about by isolating argument parsing, validation, and command execution into small units.
- Remove duplicated logic across commands (file IO, layout parsing/serialization, conflict printing, firmware layout selection).
- Break up monolithic modules (`src/bin/zmk-layout.rs`, `src/tasks/mod.rs`, `src/flash/mod.rs`, `src/providers/mod.rs`) into cohesive pieces with explicit boundaries.
- Preserve current behavior with stronger regression coverage while shrinking per-module LOC and public surface area.

## Current Pain Points
- `src/bin/zmk-layout.rs` (~1370 LOC) mixes clap definitions, business logic, IO, and printing; `run_apply`/`run_validate`/`run_diff` largely duplicate flow control and serialization.
- Task engine (`src/tasks/mod.rs`, ~2921 LOC) combines TOML parsing, validation, target normalization, conflict detection, execution, and Lua bridging in one file, making changes risky.
- Flashing (`src/flash/mod.rs`, ~2064 LOC) interleaves platform-specific probes, polling loops, and core flashing steps behind cfgs; hard to test or swap behaviors.
- Providers (`src/providers/mod.rs`, ~1428 LOC) handles layers, combos, behaviors, and formatting in a single module with repeated normalization/parsing helpers.
- IO/serialization glue (reading layouts/task files, emitting DTS/JSON, diff printing) is reimplemented per command instead of sharing a small utility layer.

## Architectural Direction
- Introduce a dedicated `cli` module tree: `cli::app` (clap definitions), `cli::context` (shared paths/config), `cli::commands::*` (task runner, script, layer, bundle, firmware, profiles). Handlers should be thin shims that call library functions.
- Add a reusable IO layer (e.g., `crate::io::{read_task_file, read_layout, write_layout, print_diff}`) consumed by CLI and tests to cut boilerplate and centralize error mapping.
- Split the task engine into `task::config` (schema/ids), `task::targets` (normalized target types + overlap checks), and `task::engine` (execution/conflicts). Move Lua execution into a dedicated `lua_engine` module (outside `task`) that exposes a minimal API the task engine can call.
- Extract `flash` into platform modules (`flash::linux`, `flash::macos`, `flash::windows`) plus `flash::core` for artifact resolution, duplicate-serial tracking, and progress reporting.
- Break provider responsibilities into submodules (`providers::layers`, `providers::combos`, `providers::behaviors`, `providers::format`) sharing a single binding normalization/formatting helper.
- Simplify adapter import/export/render with builder-style options, a shared node-rendering trait, and property-order/template helpers to cut duplication.
- Favor clean breaks over compatibility shims; adjust public APIs as needed to match the new layout without preserving legacy paths.

## Process
- Develop each phase test-first (add/extend failing tests or fixtures before implementing changes).
- Record each phase in `CHANGELOG.md` using the existing format and grouping.
- Commit at the end of every phase with a scoped message; do not batch multiple phases into one commit.

## Work Plan (sequenced)
1) **Safety net**: Expand CLI regression tests for lesser-covered paths (bundle render/import/export combos, firmware flash/device listing, script `--diff`/`--output` cases, adapter standard/moergo round trips). Capture current outputs as fixtures to guard refactors, and add focused task-engine fixtures for validation/conflict error shapes and target normalization edge cases.
2) **Shared IO utilities**: Introduce a small utility module for reading/writing DTS/JSON/task files and rendering diffs. Update CLI handlers and tests to use it, removing ad-hoc `fs::read_to_string` + manual error wrapping.
3) **CLI decomposition**: Move clap structs/enums into `cli::app`; create per-command modules with shared helpers for task loading, layout IO, and conflict printing. Limit `main` to wiring parse -> dispatch. Keep exit codes identical.
4) **Task engine split**: Carve `src/tasks/mod.rs` into parser/validator (`task::config`), target model/conflict detection (`task::targets`), and executor (`task::engine`). Extract Lua execution into a separate `lua_engine` module consumed by tasks. Add focused unit tests per submodule and keep existing integration tests green.
5) **Flash module cleanup**: Isolate platform probes/pollers into per-OS files that implement a `Discovery`/`Probe` trait consumed by `flash::core`. Gate platform-specific deps behind cargo features and provide stub backends so `cargo check --all-targets --all-features` stays green on non-host OSes. Add fakes for unit tests (single/multi-device, duplicate serials, missing board-id, timeouts, permission errors) plus golden snapshots for list/wait/progress output; feature-gate any end-to-end smoke test.
6) **Provider refactor**: Extract layer/combo/behavior helpers into dedicated files sharing normalization/formatting utilities; codify a lightweight `BindingFormat` helper to remove repeated string munging.
7) **Adapter/build dedup**: Centralize layout source handling (JSON vs DTS vs files) so firmware and layer/bundle commands reuse the same resolver. Collapse adapter import/export/render into builder-style APIs using shared node-rendering/property-order helpers plus unified template extraction/formatting hints to remove duplicate paths.
8) **Polish & docs**: Update `README.md` and CLI docs to match the new structure, document module boundaries for contributors, and remove dead code/old helpers once call sites migrate. Keep CHANGELOG entries for each milestone.

## Success Criteria
- CLI code split into clear modules with `src/bin/zmk-layout.rs` trimmed by >50% LOC and no behavior regressions in tests.
- Task/flash/provider modules reduced to focused files (<800 LOC each) with unit tests covering previously implicit behaviors.
- Task engine validation/conflict outputs captured as fixtures to flag regressions when splitting config/targets/engine.
- Flash/device discovery covered by trait-backed fakes with golden CLI output snapshots for list/wait/progress flows; non-host builds stay passing via feature-gated platform deps + stubs.
- Adapter import/export/render collapsed to unified builder-style APIs with shared rendering/property-order helpers and round-trip fixtures (standard/moergo) kept stable.
- Shared IO utilities eliminate duplicate read/parse/serialize/diff logic across commands.
- External APIs may change; new module layout and APIs become the source of truth without compatibility layers, documented as the 0.x baseline for downstreams.

## Risks & Mitigations
- Behavior regressions during module splits → rely on expanded CLI fixtures and per-module unit tests before moving code.
- API churn for downstream crates → document breaking changes clearly; prioritize simplicity over shims.
- Platform-specific flash regressions → cover discovery/polling via trait fakes + golden outputs; gate any end-to-end probe smoke tests behind feature flags.

## Progress Updates
- **Safety net** (Plan step 1): Added CLI regression coverage and fixtures for apply/validate/diff, firmware manifest/device listing, script `--diff`/`--output`, bundle import/render/export, and layer import/export with template placeholders (`tests/cli.rs` + `tests/fixtures/cli_*`). These snapshots guard the CLI surface before deeper refactors.
- **Shared IO utilities** (Plan step 2): Introduced `src/io/mod.rs` for reading/writing text, parsing layouts/tasks, and rendering unified diffs; CLI handlers now consume these helpers and the module carries a small diff-format test.
- **CLI decomposition** (Plan step 3): `src/bin/zmk-layout.rs` now defers to the `cli` module. Argument definitions live in `src/cli/app.rs`; per-command handlers sit under `src/cli/commands/*`; shared context/error handling is in `src/cli/context.rs`/`error.rs`. Command flows reuse the IO helpers and the new CLI tests.
- **Task engine split prep** (Plan step 4): Created `tasks::config`, `tasks::targets`, and `tasks::lua_engine` modules as re-export shims; the parser/types/target helpers/Lua glue still live in `engine.rs`. Completing the split cleanly will require pulling the config parsing/types, target helpers, and Lua helpers out of `engine.rs` and rewiring imports. I can proceed with that refactor next (it’s a larger change touching the task engine internals).
- **Task engine split** (Plan step 4): Extracted config parsing/types into `tasks::config`, target helpers into `tasks::targets`, and Lua helpers/script execution into `tasks::lua_engine`. `engine.rs` now focuses on execution, pulling in the helpers from the new modules; re-exports and tests were updated accordingly, and the task engine builds after the split.
- **Flash module cleanup (Phase 5 start)**: Split `flash` into `flash::core` (shared types/flow), `flash::linux`, `flash::macos`, `flash::windows`, and a stub backend for unsupported targets. Existing probe/wait logic moved into per-OS files; public API remains intact and flash tests still pass. Follow-ups: introduce platform traits + fakes for unit tests, add golden output snapshots, and gate platform deps behind features.
- **Flash module cleanup (Phase 5 complete)**: Added a `FlashBackend` trait with a default platform backend plus fake backend tests covering artifact copy, board-id verification, duplicate serial rejection, missing board-id warnings, and mount validation. Refactor keeps the public API stable while making discovery/wait injectable for tests; flash tests now run without OS dependencies. Next steps move to later phases.
