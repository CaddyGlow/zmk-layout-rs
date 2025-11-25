# KeymapDocument Refactor Plan (no backwards compatibility)

## Goal
Replace `KeymapDocument`’s DTS-centric representation with a purpose-built semantic model for keyboard layouts, and refactor consumers to operate on that model. DTS parsing/serialization remains at the edges; internal logic uses the new types exclusively.

## Core Data Model
- `KeymapDocument` (owns semantic layout):
  - `layers: Vec<Layer>`
  - `combos: Vec<Combo>`
  - `behaviors: Vec<Behavior>`
  - `macros: Vec<MacroDef>`
  - `input_listeners: Vec<InputListener>`
  - `metadata: LayoutMetadata`
- `Layer`:
  - `name: String`
  - `bindings: Vec<String>` (ordered, key-count aligned)
  - `notes: Option<String>`
- `Combo`:
  - `name: String`
  - `bindings: Vec<String>`
  - `key_positions: Vec<usize>`
  - `layers: Vec<String>`
  - `metadata: ComboMetadata` (e.g., tap/hold, timeout, condition strings)
- `Behavior`:
  - `name: String`
  - `driver: String`
  - `params: BehaviorParams` (structured, not raw DTS)
- `MacroDef`:
  - `name: String`
  - `steps: Vec<String>`
- `InputListener`:
  - `name: String`
  - `handler: String`
  - `params: ListenerParams`
- `LayoutMetadata`:
  - `includes: Vec<String>`
  - `custom_devicetree: Vec<String>`
  - `template_info: Option<TemplateInfo>`
- `TemplateInfo`:
  - `source: Option<String>` (raw template text)
  - `mode: TemplateParseMode` (reuse existing enum)
- Strong typedefs:
  - `BehaviorParams`, `ListenerParams`, `ComboMetadata` as structured maps/typed fields to avoid raw stringly data.

## Parsing/Serialization Layer
- New module `keymap_codec` with explicit conversions:
  - `fn dts_to_keymap(rendered: &str, template: Option<TemplateCtx>) -> Result<KeymapDocument>`
  - `fn keymap_to_dts(doc: &KeymapDocument, template: Option<TemplateCtx>) -> Result<String>`
  - `fn json_to_keymap(text: &str) -> Result<KeymapDocument>`
  - `fn keymap_to_json(doc: &KeymapDocument) -> Result<String>`
- `TemplateCtx` holds `{ source: String, mode: TemplateParseMode }`.
- Validation lives here (layer count vs bindings, combo references, behavior existence).

## Adapter Pipeline
- `AdapterPipeline` to produce/consume `KeymapDocument` instead of `AdapterLayout`/`DtsDocument`.
- Rename methods to signal semantic output:
  - `load_keymap()` -> returns `KeymapDocument`.
  - `from_keymap(doc: KeymapDocument)` for inverse path where needed.
- Remove `AdapterLayout` usage in pipeline; keep DTS/template handling only in codec layer.

## Layout Engine
- `LayoutEngine` to own `KeymapDocument` directly:
  - Replace `document: KeymapDocument` references (drop DTS handles internally).
  - Mutators (`set_binding`, `set_layer`, combo/behavior CRUD) operate on semantic structs.
  - Rendering methods (`render_keymap_text`, `render_standard_json`) delegate to `keymap_codec`.

## CLI / Build / IO
- `io::load_layout*` to return `KeymapDocument` (plus optional template context) instead of `LayoutHandle/DtsDocument`.
- `LayoutHandle` can be simplified or dropped; staging uses `KeymapDocument` + codec to emit DTS/JSON.
- `Firmware build` path: `LayoutSource::Document` now carries `KeymapDocument`; pipeline variant produces `KeymapDocument`.
- `Layer export/import` commands use codec functions directly; no DTS in memory except at parse/emit edges.

## Lua API
- `KeymapDocument::from_document`/`document()` are removed; expose semantic getters/setters.
- Update Lua bindings to construct `KeymapDocument` via codec (`load_dts/load_json`) and to serialize via codec (`save_dts/save_json`).
- Remove any direct DTS string parsing from Lua surface; route through codec for validation.

## Tests & Fixtures
- Golden tests for round-tripping:
  - `DTS -> KeymapDocument -> DTS` (with/without templates).
  - `JSON -> KeymapDocument -> JSON`.
  - Ensure lossless ordering for layers/bindings/combos and preserved semantics.
- Update adapter pipeline/tests to assert `KeymapDocument` usage and template handling.
- Update layout engine tests to operate on semantic structs only.

## Migration Steps (no compatibility layer)
1) Implement new semantic structs and `keymap_codec`.
2) Switch `AdapterPipeline` to `KeymapDocument`.
3) Convert `LayoutEngine` and helpers to own `KeymapDocument`.
4) Update `io` loaders and `LayoutSource` variants to carry `KeymapDocument`.
5) Refactor CLI commands (`layer`, `firmware`, `bundle`, `script`) to use the new types.
6) Update Lua bindings to the semantic API.
7) Refresh docs/examples to reflect the new surface.
8) Add golden round-trip tests and fix existing tests to match new types.
