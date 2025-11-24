# LayoutHandle Unification Plan

Goal: replace the current ad-hoc layout representations (`LoadedLayout`, `KeymapDocument` wrappers, `AdapterLayout` usage, `LayoutSource` + `KeymapArtifacts`) with a single `LayoutHandle` that tracks source text, parse outputs, and template/profile context. This should also (1) make staging deterministic for JSON/pipeline inputs by always producing keymap/config artifacts and (2) make preprocessing vs raw text explicit for diffs.

## Target API
- `pub struct LayoutHandle {`
  - `source_path: Option<PathBuf>` // where the input came from (file), if any
  - `raw_text: Option<String>` // on-disk text as read; always populated for file-based inputs
  - `preprocessed_text: Option<String>` // only set when preprocessing was requested
  - `document: DtsDocument` // parsed AST; always present
  - `adapter_layout: Option<AdapterLayout>` // cached standard JSON view, built lazily
  - `profile: Option<KeyboardProfileDoc>` // if known (firmware build path)
  - `template_source: Option<String>` // resolved template used for JSON<->DTS conversions
  - `template_mode: TemplateParseMode` // strip vs full document, default strip placeholders
  - `origin: LayoutOrigin` // enum: DtsFile, DtsText, JsonFile, JsonText, Pipeline
`}`
- Core methods:
  - `LayoutHandle::from_dts_path(path, preprocess?: Option<PreprocessorConfig>) -> Result<Self, IoError>`
  - `LayoutHandle::from_dts_text(text, template_ctx?) -> Result<Self, IoError>`
  - `LayoutHandle::from_json_path(path, template_ctx?) -> Result<Self, IoError>` // parses JSON into AdapterLayout, applies template (if present) to build DtsDocument
  - `LayoutHandle::from_json_text(text, template_ctx?) -> Result<Self, IoError>`
  - `LayoutHandle::from_pipeline(pipeline: AdapterPipeline, template_ctx?) -> Result<Self, IoError>`
  - `fn raw_for_diff(&self) -> (&str, bool /*is_preprocessed*/)` // returns best-effort text + flag to warn
  - `fn as_adapter_layout(&mut self) -> Result<&AdapterLayout, AdapterError>` // caches
  - `fn as_keymap_document(&self) -> KeymapDocument` // thin wrapper around `document.clone()`
  - `fn render_keymap_text(&mut self) -> Result<String, AdapterError>` // uses document.to_string()
  - `fn render_standard_json(&mut self) -> Result<String, AdapterError>` // via cached adapter_layout
  - `fn to_artifacts(&mut self, workspace: &WorkspaceHandle, profile: Option<&KeyboardProfileDoc>) -> Result<KeymapArtifacts, BuildError>` // writes layout.json + keymap.dtsi (+ config if profile/layout template requires)

## Integration Steps
1) **Introduce types/utilities**
   - Add `LayoutOrigin` enum and `LayoutHandle` struct in a new module (e.g., `src/layout_handle.rs`).
   - Move/rename `LoadedLayout` to this new struct; keep old name as a deprecated shim temporarily (behind a feature flag or type alias) to ease incremental migration.
   - Provide helpers for template context (profile root resolution, template_mode).

2) **Refactor IO/parsing**
   - Update `io::load_layout`/`load_layout_preprocessed` to return `LayoutHandle` instead of `LoadedLayout`.
   - Preserve both `raw_text` and `preprocessed_text` when preprocessing is used.
   - Adjust `serialize_keymap` to accept `KeymapDocument` or `LayoutHandle` and clarify expectations.

3) **CLI/task consumers**
   - `cli/context.rs`, `cli/commands/tasks.rs`, `cli/commands/script.rs`: swap to `LayoutHandle`, extract `raw_for_diff()` for diff printing. If `is_preprocessed`, print a warning: “diff is against preprocessed content”.
   - When building `KeymapDocument`, call `handle.as_keymap_document()`.

4) **Adapter/pipeline reuse**
   - Update `adapters/pipeline.rs` to expose a `to_layout_handle(template_ctx)` helper that populates `adapter_layout` directly, avoiding double parse.
   - `adapters/standard/io.rs`: reuse `LayoutHandle::render_standard_json` when exporting; allow feeding `LayoutHandle` instead of raw doc.

5) **Firmware build flow**
   - Normalize `LayoutSource` -> `LayoutHandle` early in `BuildRequestBuilder` or inside `LayoutStager`.
   - In `LayoutStager::stage`, call `handle.to_artifacts(...)`:
     - For JSON/pipeline origins: render DTS via template/profile, write `keymap.dtsi`, also emit `layout.json`.
     - For DTS origins: write `keymap.dtsi` from `document`; optionally emit `layout.json` using `as_adapter_layout`.
   - Ensure both toolchains always receive a `keymap` path; config generation rules can piggyback on profile/template data when available.

6) **Deprecation/cleanup**
   - Remove `LoadedLayout` once callers are migrated.
   - Keep `KeymapArtifacts` but document that it’s produced exclusively by `LayoutHandle::to_artifacts`.
   - Reduce direct `AdapterLayout::from_document` calls in favor of `handle.as_adapter_layout`.

7) **Testing**
   - Add tests covering:
     - JSON file -> `LayoutHandle` -> artifacts includes `keymap.dtsi` (fixes missing keymap issue).
     - DTS text -> `LayoutHandle` -> standard JSON roundtrip retains metadata/template.
     - Preprocessed input: `raw_for_diff` flag triggers warning and diff text matches expanded content.
     - Firmware staging for each `LayoutSource` variant yields keymap/config paths and passes toolchain requirements.

## Empty layout constructor
- Add `LayoutHandle::empty(profile: &KeyboardProfileDoc) -> Result<Self, Error>` (or `from_profile_template`):
  - Resolve the profile template, render a minimal DTS (behaviors/macros/combos roots + predictable base layer), set `origin = LayoutOrigin::Generated`.
  - Populate `raw_text` with the rendered template, set `template_source`/`template_mode`, parse into `document`, and seed `adapter_layout` for JSON export.
  - Skip preprocessing for generated content; enforce a warning/error if preprocessing is requested with no source path.
- Use this in CLI/task flows when no layout is provided, and in firmware builds only when a profile is available; otherwise fail fast.

## Expected Outcomes
- Single, explicit source of truth for “layout in memory” with clear provenance.
- Deterministic staging for all input types (no more missing keymap when starting from JSON/pipeline).
- Clear UX around preprocessing vs on-disk text when showing diffs.
