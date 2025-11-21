Goal: replace direct DTS handling with a single JSON “layout bundle” that keeps the normalized layout plus build/context metadata. No backward-compat layer; the bundle becomes the source of truth. Current DTS/JSON flows become shims onto the bundle.

## Scope (what we ship)

- Bundle schema with validation: `format_version`, metadata, `layout` (existing AdapterLayout shape), `overlays` (custom DTS fragments), `symbols` (includes/defines/search paths/template vars), `targets` (firmware/toolchain/template selection), `sources` (provenance).
- MoErgo bundle adapter: import/export MoErgo JSON while preserving overlays/symbols and enabling round-trips to the MoErgo editor.
- Rendering pipeline: render DTSI from a bundle by target (`includes`/`defines`/template applied). DTSI is an artifact, not the source of truth.
- CLI: bundle-first commands (`import moergo`, `export moergo`, `render`). Legacy DTS/standard JSON paths shim through the bundle internally.
- Fixtures/tests/docs: cover MoErgo round-trip, target-based renders, and schema validation.

Out of scope for this pass: QMK/other firmware adapters, long migration timelines, backward compatibility modes.

## Phases

1. **Schema & validation**
   - Add `LayoutBundle` struct with serde + validation (required keys, target references, overlays/symbol names).
   - Keep `layout` = existing `AdapterLayout` (no binding schema changes).
   - Add fixtures: minimal bundle; MoErgo bundle with overlays/targets.
   - Tests: schema round-trip; validation errors for missing targets/overlays.

2. **MoErgo bundle adapter**
   - Import MoErgo JSON → `LayoutBundle`: map MoErgo layout into `layout`; stash `custom_devicetree`, `custom_defined_behaviors`, `custom_macros`, `input_listeners`, `key_position_header` into `overlays`; collect includes/defines into `symbols`; create default `target` for MoErgo firmware/template; record provenance in `sources`.
   - Export bundle → MoErgo JSON: rehydrate overlays/symbols back into MoErgo fields; strip bundle-only metadata.
   - Tests: round-trip TailorKey sample; assert overlays/symbols land correctly.

3. **Rendering pipeline**
   - Implement `bundle.render(target_id, template_override?)` using existing template engine, applying target `includes`/`defines` and overlays.
   - CLI: `zmk-layout bundle import moergo`, `bundle export moergo`, `bundle render --target <id> --output keymap.dts`. Legacy `export/import standard`/`dts` go through bundle internally.
   - Tests: render MoErgo target vs a generic ZMK target with differing includes/defines; ensure DTSI is deterministic.

4. **Docs & polish**
   - Document bundle schema and CLI usage; note that bundle is the new source of truth.
   - Add CHANGELOG entry once code lands.
   - Add fixtures/examples showing MoErgo import → render → MoErgo export flow.

## File hits (expected)

- `src/adapters/` (new bundle structs + MoErgo adapter integration)
- `src/adapters/standard/` (reuse AdapterLayout; thin shims)
- `src/serialization/render` or equivalent (bundle render)
- `src/bin/zmk-layout.rs` (CLI wiring)
- `tests/` + `examples/` (bundle fixtures; MoErgo round-trip)
- `docs/` + `CHANGELOG.md`

## Risks/Rules

- Do not alter AdapterLayout/binding shape; keep current JSON format internally.
- DTSI remains an artifact; all writes go through bundle.
- No backward-compat modes; legacy commands internally lift to bundle.
