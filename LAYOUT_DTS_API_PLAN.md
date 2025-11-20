# Layout & DTS Lua API Completion Plan

## Goals
- Expose full layout/document CRUD to Lua (DTS + standard JSON)
- Finish fluent builders so they mutate the document (no placeholders)
- Provide in-memory parse/render helpers for both DTS and JSON with templates
- Add coverage for metadata, ordering, deletes, inputs, conditionals, behaviors, macros
- Keep Lua ergonomics 1-based, immutable snapshots, explicit apply lifecycle

## Scope
1. Root-level helpers: meta, layer ordering, delete operations
2. Behavior/Combo/Input/Conditional: complete apply paths & queries
3. Serialization: string/file load+save for DTS/JSON, template-aware options
4. Query: richer info objects (metadata/properties/layers/inputs/conditionals)
5. Validation & error handling: deterministic Lua-facing errors
6. Tests: unit+integration (Lua) for end-to-end mutations and I/O

## Task Breakdown

### 1) Root-Level Helpers
- Add `layout:meta(key, value)` and `layout:get_meta(key)` to edit/read global meta nodes.
- Add `layout:remove_layer(name)`; update effects on layer defines if applicable.
- Add `layout:move_layer(name, index|before|after)` (1-based).

### 2) Builders: Complete Apply Paths
- **LayerBuilder**: support delete via root helpers; ensure metadata writes stay.
- **ComboObject**: support `on_layers` and `when` in apply; add delete via `layout:remove_combo(name)`.
- **BehaviorObject**: write params/bindings into DTS (no placeholders) and support delete.
- **MacroObject**: implement action encoding into behaviors/macros; add delete.
- **InputObject**: actually write encoder/sensor nodes (resolution, bindings); add delete.
- **ConditionalObject**: write conditional layer nodes; add delete.
- Enforce staged→applied lifecycle with clear errors; allow re-acquire to edit existing definitions.

### 3) Serialization Helpers
- String-based: `layout:to_dts_string()`, `layout:to_json_string(template_path)`; `layout:parse_dts(source)`, `layout:parse_json(json, template_path)`.
- File-based: `layout:load_dtsi(path)`, `layout:save_dtsi(path)`, `layout:load_json(json_path, template_path)`, `layout:save_json(path[, template_path?])`.
- Template-aware rendering: `layout:render_template(json, template_path)` returning DTS string.

### 4) Query Expansion
- **LayerInfo**: include metadata table (read-only), size, index.
- **ComboInfo**: include layers/conditions/description/properties; expose delete flag? (optional).
- **BehaviorInfo**: include properties map (#binding-cells, compatible, label, etc.).
- **InputInfo/ConditionalInfo**: add info objects and list/get APIs.
- List functions per type: `list_inputs()`, `list_conditionals()`, `list_macros()`.

### 5) Validation & Errors
- Keep Lua 1-based indices; convert centrally.
- Deterministic error strings for: invalid type, out-of-range, missing apply, double-apply, missing template, parse failures.
- Read-only snapshots via metatable proxies.

### 6) Tests
- Unit tests covering: index conversion, staged→applied, delete, move_layer, metadata, read-only snapshots, serialization helpers.
- Integration Lua tests: round-trip DTS/JSON with mutations; create/edit/delete combos/behaviors/macros/inputs/conditionals; move layers; validate metadata; template render path.
- Error-path tests: invalid indices/types, double-apply, missing bindings/keys, missing template.

## File Targets
- `src/lua_api/api.rs`: new root helpers (meta, delete, move), serialization functions; register new query/list functions.
- `src/lua_api/layer.rs`: ensure metadata/write path supports deletes? (root helper).
- `src/lua_api/combo.rs`, `behavior.rs`, `macro_builder.rs`, `input.rs`, `conditional.rs`: implement real apply; add deletes if builder-based, otherwise root helpers.
- `src/lua_api/query.rs`: expand info objects & list/get for inputs/conditionals/macros; include metadata/properties.
- `src/lua_api/util.rs`: shared conversions/errors.
- `src/tasks/mod.rs`: ensure registration includes new methods.
- Tests: `tests/lua_api.rs` expanded with fixtures/templates; possible new fixtures under `tests/fixtures/`.

## Success Criteria
- Lua can load/parse DTS or JSON (with template), mutate all constructs, and save/render back to DTS/JSON.
- All builders apply mutations (no placeholders); delete/move supported.
- Query APIs return rich, read-only snapshots.
- Tests cover CRUD, ordering, serialization, and error paths.
