# Keyboard Profile Migration Plan

We now have a TOML-based keyboard profile specification (`docs/keyboard_profiles.md`)
and a first profile (`profiles/keyboards/glove80.toml`). This plan outlines how to
move the codebase from the legacy YAML folders under `example_profile/` to the
new unified format without breaking existing tooling.

## Phase 1 – Loader & Data Model
1. **Define data structures** mirroring the TOML schema: `KeyboardProfileDoc`,
   nested `Metadata`, `Hardware`, `Firmware`, `Layout`, and lists for behaviors,
   combos, macros. Keep the types in a new module (e.g., `src/profiles/mod.rs`)
   so other crates can consume them.
2. **Implement a parser** that loads TOML files, validates required sections,
   normalizes optional fields (default headers, empty arrays), and emits
   actionable errors (missing `keyboard`, invalid row lengths, etc.).
3. **Unit tests**: golden fixtures for a valid profile, missing-field errors,
   extra data passthrough, and layout formatting rows that follow the schema
   documented in `docs/keyboard_profiles.md` (array-of-table representation).
4. **Update & commit**: document the completed loader work in this plan and
   create a git checkpoint before starting Phase 2.
> Status: Completed – added `src/profiles/mod.rs`, `profiles/keyboards/glove80.toml`
> fixture updates, and loader tests covering happy-path parsing and validation.

## Phase 2 – Manifest Integration
1. **Augment `FirmwareManifest`** (`src/build/manifest.rs`) with an optional
   `keyboard_profile_path` and/or parsed `KeyboardProfileDoc` reference.
   Hydrate it when `metadata.profile` exists so downstream code can rely on a
   single source of truth.
2. **Extend `BuildRequest` / CLI** to expose keyboard metadata pulled from the
   profile (name, vendor, firmware defaults). When users run `zmk-layout
   firmware build --keyboard glove80`, echo info from the profile instead of
   duplicating strings in the manifest.
3. **Backwards compatibility**: tolerate manifests that still point to legacy
   YAML by logging a warning and skipping profile hydration. Once all keyboards
   migrate we can drop the fallback.
4. **Update & commit**: record the manifest changes in the plan and create a git
   commit before moving to Phase 3.
> Status: Completed – manifest/CLI now hydrate `KeyboardProfileDoc` instances,
> expose metadata in `BuildRequest`, and warn when legacy YAML references remain.

## Phase 3 – Layout & Template Consumers
1. **Adopt the loader in layout/template helpers** (e.g.,
   `import_standard_file_with_template` and documentation examples) so utilities
   that previously embedded YAML snippets can hydrate settings from
   `profiles/keyboards/*.toml`.
2. **Expose layout metadata** (formatting rows, template path, key position
   header) through helper APIs so `import_standard_file_with_template` or other
   generators can pull them directly.
3. **Add tests** exercising the layout metadata to guarantee keymap rendering
   stays consistent (e.g., snapshot of rendered ASCII rows).
4. **Update & commit**: summarize the layout/template work in this file and
   checkpoint the repository before Phase 4.
> Status: Completed – added adapter helpers that consume keyboard profiles,
> layout metadata helpers (`ascii_art`, key position headers), and regression tests.

## Phase 4 – Cleanup & Documentation
1. **Remove `example_profile/`** once all keyboards have TOML profiles, their
   template references have been moved, and no code references the old directory.
2. **Refresh documentation**: README, `docs/firmware_building.md`, and any other
   guides should link to the TOML spec and example files.
3. **Changelog entry** describing the migration and any CLI behavior changes,
   especially if the profile loader becomes mandatory.
4. **Update & commit**: finalize this plan with the cleanup status and create
   the final git commit to close out the migration.
> Status: Completed – removed the legacy `example_profile/` tree, relocated the
> Glove80 template under `templates/`, refreshed documentation/README snippets,
> described the migration in the changelog, and added the `zmk-layout profiles
> check` command (with `--all`/`--profiles-dir`) so CI/users can validate TOML
> files directly.

## Risks & Mitigations
- **Incomplete profiles**: Add a validation command (e.g., `zmk-layout profiles
  check`) so CI can catch missing sections before runtime.
- **Breaking existing workflows**: keep the loader optional until every keyboard
  manifest points at a TOML profile, and gate new functionality behind a feature
  flag if necessary.
- **Spec drift**: version the profile schema (already at `version = 1`) and keep
  fixtures under `tests/fixtures/profiles/` so future changes require explicit
  updates.
