# Fluent Lua API Implementation Plan

## Overview

Implement a comprehensive fluent, object-oriented API for Lua scripts that allows:
- Creating and editing all ZMK constructs (layers, combos, behaviors, macros, inputs, conditionals)
- Querying existing layout state
- Reusing objects directly as bindings
- Modifying existing layouts (not just creating from scratch)

This creates a more readable, maintainable scripting experience compared to the current function-based API.

## Goals

- Create chainable builder objects for ALL ZMK constructs
- Allow objects to be used directly as bindings (combo, macro, behavior references)
- Support both creation and modification of existing layout elements
- Query API to inspect current layout state
- Maintain backwards compatibility with existing function-based Lua API
- Provide type-safe Rust implementations using mlua UserData
- Enable lazy evaluation with auto-apply semantics

## Current State (post-refactor)

- The `layout` global is installed from `tasks::lua_engine::register_script_api`, pointing at `LayoutApi` under `src/lua_api`.
- Layer/combo/behavior builders mutate the document; macro/input/conditional builders still stage data only (no document writes), and combo/behavior builders don’t preload existing definitions when editing.
- Query objects cover layers/combos/behaviors but omit metadata, layer order, filters, and staged builders default to empty values when reading.
- Layout-level I/O helpers exist for DTS/JSON files plus `parse_dts`; string-based renders/imports and template overrides are not yet exposed.
- Error handling uses ad-hoc runtime errors via `script_error`; only `LayerBuilder` enforces double-apply; index/type validation is not centralized for every builder.

## Commit Strategy

**IMPORTANT**: Each phase must be committed separately with a corresponding CHANGELOG.md entry following the established format. After each phase completion:

1. Run all tests to ensure nothing breaks
2. Update CHANGELOG.md with phase summary
3. Create a git commit with descriptive message
4. Continue to next phase
5. No phase is “done” until tests + CHANGELOG + git commit are complete

---

## Complete API Design

### Core Concepts

1. **Builder Objects**: Fluent API for creating/modifying ZMK constructs
2. **Query API**: Read existing layout state
3. **Reusable References**: Use objects directly as binding strings
4. **Lazy Evaluation**: Apply changes only when needed (first use or explicit apply())
5. **Edit vs Create**: Support modifying existing definitions
6. **Lua 1-Based Indexing**: All Lua-facing indices (bindings, key positions) are 1-based; conversions to internal zero-based layouts happen inside Rust
7. **Immutable Queries**: Query methods return snapshots (copies) that must not be mutated to change state
8. **Apply Semantics**: Builders auto-apply on first use unless explicitly applied; once applied, objects are immutable—re-acquire a builder to edit again
9. **Ownership Discipline**: Rust `UserData` wraps staged state with interior mutability carefully (no nested mutable borrows); avoid `Send` unless thread-safe

---

## API Reference

### 1. LayoutAPI (Global: `layout`)

Main entry point provided as global `layout` object in Lua scripts.

**Creation Methods**:

```lua
-- Create new or get existing layer builder
layout:layer(name)           -- LayerBuilder

-- Create new combo
layout:combo(name)           -- ComboObject

-- Create new or modify existing behavior
layout:behavior(name)        -- BehaviorObject

-- Create new macro
layout:macro(name)           -- MacroObject

-- Create new input listener
layout:input(name)           -- InputObject

-- Create new conditional layer
layout:conditional(name)     -- ConditionalObject
```

**Query Methods**:

```lua
-- Query existing layout state
layout:get_layer(name)       -- LayerInfo (read-only)
layout:get_combo(name)       -- ComboInfo (read-only)
layout:get_behavior(name)    -- BehaviorInfo (read-only)
layout:list_layers()         -- string[]
layout:list_combos()         -- string[]
layout:list_behaviors()      -- string[]

-- File/string I/O helpers (file-based ones exist today; string renders/imports are planned)
layout:load_dtsi(path)
layout:save_dtsi(path)
layout:load_json(json_path, template_path)
layout:save_json(path[, template_path])
layout:parse_dts(source)
layout:parse_json(json, template_path)
layout:to_dts_string()
layout:to_json_string(template_path)
layout:render_template(json, template_path)
```

---

### 2. LayerBuilder

**Purpose**: Create new layer or edit existing layer bindings/metadata.

**Methods**:

```lua
layer = layout:layer("base")

-- Set individual binding (creates/updates)
layer:bind(index, binding)   -- index: number (1-based), binding: string | ComboObject | MacroObject

-- Set all bindings at once (replaces entire layer)
layer:bindings(array)        -- array: (string|object)[] (1-based order)

-- Add metadata
layer:meta(key, value)

-- Query current state (before modifications)
layer:get_binding(index)     -- string (1-based)
layer:get_bindings()         -- string[] (1-based snapshot, copy)
layer:get_meta(key)          -- string

-- Terminal operation
layer:apply()                -- Apply changes
```

**Example - Edit Existing Layer**:

```lua
-- Get existing layer and modify it
local base = layout:layer("base")
local current = base:get_binding(1)  -- Read current binding at index 1 (Lua 1-based)
log("Current binding: " .. current)

-- Modify specific indices
base:bind(1, "&kp Q")
    :bind(2, "&kp W")
    :apply()

-- Or replace all bindings
base:bindings({"&kp Q", "&kp W", "&kp E", "&kp R"})
    :apply()
```

---

### 3. ComboObject

**Purpose**: Define combo that can be reused as binding reference.

**Methods**:

```lua
combo = layout:combo("esc")

-- Configuration (all chainable)
combo:keys(positions)        -- positions: number[] (1-based) (REQUIRED)
combo:binding(binding)       -- binding: string (REQUIRED)
combo:timeout(ms)            -- ms: number (optional)
combo:on_layers(layers)      -- layers: string[] (optional)
combo:when(condition)        -- condition: string (optional)

-- Query (if editing existing combo)
combo:get_keys()             -- number[] (1-based snapshot)
combo:get_binding()          -- string
combo:get_timeout()          -- number | nil

-- No explicit apply() needed - auto-applies on first use as binding; once applied, the object is immutable (re-acquire via layout:combo(name) to edit)
```

**Example - Create and Reuse**:

```lua
-- Create combo (not applied yet)
local esc = layout:combo("esc_combo")
    :keys({1, 2})
    :binding("&kp ESC")
    :timeout(50)

-- Use in layer (auto-applies combo)
layout:layer("base")
    :bind(10, esc)           -- Combo automatically applied here
    :apply()

-- Reuse in another layer
layout:layer("nav")
    :bind(5, esc)            -- Reuses same combo reference
    :apply()
```

**Example - Edit Existing Combo**:

```lua
-- Get existing combo and modify it
local esc = layout:combo("existing_combo")
local current_keys = esc:get_keys()
log("Current keys: " .. table.concat(current_keys, ", "))

-- Modify and reapply
esc:keys({1, 2, 3})          -- Change key positions
   :timeout(100)             -- Change timeout
   :apply()                  -- Must explicitly apply when editing
```

---

### 4. BehaviorObject

**Purpose**: Configure ZMK behavior parameters and bindings.

**Methods**:

```lua
behavior = layout:behavior("sticky_key")

-- Set parameter
behavior:param(key, value)   -- value: any (string, number, bool)

-- Set behavior bindings
behavior:bindings(array)     -- array: string[]

-- Query existing values
behavior:get_param(key)      -- any
behavior:get_bindings()      -- string[] (1-based snapshot)

-- Apply changes
behavior:apply()
```

**Example - Configure Behavior**:

```lua
-- Configure sticky key behavior
layout:behavior("sticky_key")
    :param("quick-release", true)
    :param("release-after-ms", 500)
    :apply()

-- Configure hold-tap
layout:behavior("home_row_mods")
    :param("tapping-term-ms", 200)
    :param("flavor", "tap-preferred")
    :param("quick-tap-ms", 150)
    :apply()

-- Modify tap-dance bindings
layout:behavior("td0")
    :bindings({"&kp ESC", "&kp TAB"})
    :apply()
```

**Example - Edit Existing Behavior**:

```lua
-- Get existing behavior and modify
local sticky = layout:behavior("sticky_key")
local current_timeout = sticky:get_param("release-after-ms")

-- Update timeout
sticky:param("release-after-ms", current_timeout + 100)
      :apply()
```

---

### 5. MacroObject

**Purpose**: Define ZMK macros that can be used as bindings.

**Methods**:

```lua
macro = layout:macro("shrug")

-- Add macro actions (chainable)
macro:press(keys)            -- keys: string | string[]
macro:release(keys)
macro:tap(keys)
macro:wait(ms)               -- ms: number
macro:wait_release()
macro:wait_tap()

-- Query existing actions
macro:get_actions()          -- table[]

-- No explicit apply() - auto-applies on first use; once applied, becomes immutable (re-acquire to re-edit)
```

**Example - Create Macro**:

```lua
-- Simple tap sequence
local shrug = layout:macro("shrug")
    :tap("FSLH")
    :tap("BSLH")
    :wait(100)
    :tap("LPAR")
    :tap("RPAR")

-- Complex press/release sequence
local ctrl_c = layout:macro("ctrl_c")
    :press("LCTRL")
    :tap("C")
    :release("LCTRL")

-- Use as binding
layout:layer("base")
    :bind(10, shrug)         -- Auto-applies macro
    :bind(11, ctrl_c)
    :apply()
```

**Example - Multi-key Tap**:

```lua
-- Tap multiple keys at once
local combo_macro = layout:macro("special")
    :tap({"LSHIFT", "A"})    -- Tap both together
    :wait(50)
    :tap("B")
```

---

### 6. InputObject

**Purpose**: Configure input listeners (encoders, sensors, etc.).

**Methods**:

```lua
input = layout:input("encoder_1")

-- Configure input
input:type(type_str)         -- "encoder", "sensor", etc.
input:on_turn_cw(binding)    -- binding: string | object
input:on_turn_ccw(binding)
input:on_press(binding)
input:resolution(value)      -- number

-- Query
input:get_type()             -- string
input:get_cw_binding()       -- string
input:get_ccw_binding()      -- string

-- Apply
input:apply()
```

**Example - Configure Encoder**:

```lua
-- Volume control encoder
layout:input("encoder_1")
    :type("encoder")
    :on_turn_cw("&kp C_VOL_UP")
    :on_turn_ccw("&kp C_VOL_DN")
    :on_press("&kp C_MUTE")
    :resolution(2)
    :apply()

-- Scroll encoder
layout:input("encoder_2")
    :type("encoder")
    :on_turn_cw("&kp UP")
    :on_turn_ccw("&kp DOWN")
    :apply()
```

---

### 7. ConditionalObject

**Purpose**: Create conditional layer activations.

**Methods**:

```lua
cond = layout:conditional("gaming_mode")

-- Configure condition
cond:condition(expr)         -- expr: string (condition expression)
cond:then_layer(layer)       -- layer: string (activate if true)
cond:else_layer(layer)       -- layer: string (activate if false)

-- Query
cond:get_condition()         -- string
cond:get_then_layer()        -- string
cond:get_else_layer()        -- string | nil

-- Apply
cond:apply()
```

**Example - Conditional Layers**:

```lua
-- Activate different layer based on USB/Bluetooth
layout:conditional("connection_type")
    :condition("USB_CONNECTED")
    :then_layer("usb_layer")
    :else_layer("bt_layer")
    :apply()

-- Gaming mode toggle
layout:conditional("gaming_mode")
    :condition("GAMING_MODE_ACTIVE")
    :then_layer("gaming")
    :apply()
```

---

### 8. Query API (Read-Only Information)

**Purpose**: Inspect existing layout state without modification.
All query functions return immutable snapshots (copies). Mutating returned tables does not change layout state; use builders to apply edits.

**LayerInfo Object**:

```lua
local info = layout:get_layer("base")
if info then
    local bindings = info:bindings()     -- string[] (1-based snapshot)
    local name = info:name()             -- string
    local meta = info:metadata()         -- table
    log("Layer has " .. #bindings .. " bindings")
end
```

**ComboInfo Object**:

```lua
local info = layout:get_combo("esc")
if info then
    local keys = info:keys()             -- number[] (1-based snapshot)
    local binding = info:binding()       -- string
    local timeout = info:timeout()       -- number | nil
    log("Combo uses keys: " .. table.concat(keys, ", "))
end
```

**List Functions**:

```lua
-- List all layers
local layers = layout:list_layers()      -- string[]
for _, name in ipairs(layers) do
    log("Found layer: " .. name)
end

-- List all combos
local combos = layout:list_combos()      -- string[]

-- List all behaviors
local behaviors = layout:list_behaviors() -- string[]
```

---

## Usage Patterns

### Pattern 1: Create New Layout Elements

```lua
-- Create combos
local esc = layout:combo("esc"):keys({1, 2}):binding("&kp ESC")
local copy = layout:combo("copy"):keys({10, 11}):binding("&kp LC(C)")

-- Create layer using combos
layout:layer("base")
    :bindings({
        "&kp Q", "&kp W", esc, copy, "&kp E", "&kp R",
        "&kp A", "&kp S", "&kp D", "&kp F", "&kp G"
    })
    :meta("display-name", "QWERTY Base")
    :apply()
```

### Pattern 2: Edit Existing Layout

```lua
-- Read current state
local base = layout:layer("base")
local current_bindings = base:get_bindings()
log("Current layer has " .. #current_bindings .. " bindings")

-- Modify specific bindings
base:bind(1, "&kp A")       -- Change first binding
    :bind(2, "&kp B")       -- Change second binding
    :apply()

-- Or query and conditionally modify
for i = 1, #current_bindings do
    local binding = base:get_binding(i)
    if binding == "&trans" then
        base:bind(i, "&kp SPACE")  -- Replace all transparent keys
    end
end
base:apply()
```

### Pattern 3: Copy and Modify

```lua
-- Get existing layer bindings
local base_info = layout:get_layer("base")
local base_bindings = base_info:bindings()

-- Create new layer based on existing
local modified = {}
for i, binding in ipairs(base_bindings) do
    if binding:match("&kp") then
        table.insert(modified, binding)
    else
        table.insert(modified, "&trans")
    end
end

layout:layer("filtered")
    :bindings(modified)
    :apply()
```

### Pattern 4: Conditional Modifications

```lua
-- Check if combo exists before creating
local esc_info = layout:get_combo("esc")
if not esc_info then
    -- Create if doesn't exist
    layout:combo("esc")
        :keys({1, 2})
        :binding("&kp ESC")
else
    -- Modify if exists
    local esc = layout:combo("esc")
    esc:timeout(100)  -- Increase timeout
       :apply()
end
```

### Pattern 5: Bulk Operations

```lua
-- Apply same macro to multiple layers
local shrug = layout:macro("shrug")
    :tap("FSLH"):tap("BSLH"):tap("LPAR"):tap("RPAR")

local layers = {"base", "symbols", "nav"}
for _, layer_name in ipairs(layers) do
    layout:layer(layer_name)
        :bind(30, shrug)  -- Add macro to position 30 in each layer
        :apply()
end
```

---

## Rust Implementation Notes

- **Lifecycle State**: Each builder keeps `Staged` vs `Applied`; mutation methods require `Staged`, `apply()` flips to `Applied`, and further mutation returns a clear Lua error. Re-acquire via `layout:<object>(name)` to get a fresh staged editor seeded from current state.
- **Auto-Apply Hook**: When a builder is passed into another API (e.g., `layer:bind`), ensure it auto-applies first and reject double-applies with a specific error.
- **Index Conversion**: Central helpers convert Lua 1-based indices to internal 0-based, performing bounds checks and emitting Lua-facing indices in errors/logs.
- **Query Snapshots**: Convert internal state to deep copies before returning to Lua; optionally wrap with read-only metatables to error on mutation attempts.
- **Ownership/Threading**: Use interior mutability (`Rc<RefCell<_>>` or `Arc<Mutex<_>>` as needed) to avoid nested borrow panics in mlua callbacks; do not mark `UserData` as `Send`/`Sync` unless the internals are truly thread-safe.
- **Validation**: Validate input shapes (dense 1-based arrays, integer indices, correct types) and keep deterministic, Lua-oriented error messages.
- **Testing**: Unit-test lifecycle (staged→applied→immutable), auto-apply, index conversion, snapshot immutability, and mixed old/new API integration.

---

## Implementation Phases (post-refactor)

### Phase 1: Harden builders & lifecycle

- Seed builders from existing definitions so edits start from the current document (combos/behaviors/macros/inputs/conditionals).
- Finish apply paths for macros/inputs/conditionals and ensure combo layer filters/conditions/timeouts plus behavior bindings write through `LayoutEngine`.
- Centralize 1-based index/type validation and enforce staged→applied errors for every builder; auto-apply only when consumed as bindings; double-apply throws a clear Lua error.
- Files: `src/lua_api/{combo,behavior,macro_builder,input,conditional,layer,util}.rs`, `src/layout_engine/*` (helpers), `tests/lua_api.rs`.

### Phase 2: Layout-level helpers & serialization

- Add layout-level metadata helpers plus delete/move APIs with dependent cleanup/reindexing for combos/conditionals/macros/inputs/behaviors.
- Wire DTS/JSON string+file helpers through `adapters::pipeline` and `io` utilities (template overrides, canonical ordering) and expose template render convenience.
- Files: `src/lua_api/api.rs`, `src/io/mod.rs`, `src/adapters/pipeline.rs`, `src/layout_engine/*`, `tests/lua_api.rs`, fixtures.

### Phase 3: Query expansion

- Expand info objects to include metadata, indices, layer filters/conditions/resolution/params; add list/get coverage for macros/inputs/conditionals and seed builders from document state.
- Keep query snapshots read-only with deterministic mutation errors; ensure get_* on builders reflect document state, not staged defaults.
- Files: `src/lua_api/query.rs`, `src/lua_api/*` (builder getters), `tests/lua_api.rs`.

### Phase 4: Integration, tests, docs

- Update `tasks::lua_engine::register_script_api` to expose the full surface; add Lua fixtures covering CRUD, serialization, delete/move, and error paths; integrate with CLI regression suite.
- Publish documentation/examples (`docs/fluent_lua_api.md`, `examples/fluent_api_*.lua`) and update README + CHANGELOG; commit per phase.
- Files: `src/tasks/lua_engine.rs`, `docs/*`, `examples/*`, `CHANGELOG.md`, `tests/fixtures/*`.

## Error Handling

All builder objects validate input and provide clear error messages (runtime paths return `nil, "ERR_CODE: message"`, while programmer misuse like double-apply throws):

```lua
-- Missing required fields
local combo = layout:combo("bad")
combo:keys({1, 2})  -- Missing binding
-- Error: ERR_MISSING_FIELD: combo 'bad' requires a binding before use

-- Invalid binding type
layout:layer("base"):bind(1, 123)
-- Error: ERR_INVALID_TYPE: binding must be string or object, got number

-- Index out of range
layout:layer("base"):bind(0, "&kp Q")
-- Error: ERR_OUT_OF_RANGE: binding index must be >= 1 (Lua 1-based)

-- Double apply on immutable object
local combo = layout:combo("esc"):keys({1,2}):binding("&kp ESC")
layout:layer("base"):bind(1, combo):apply()  -- Auto-applies combo
combo:timeout(50):apply()  -- Error: ERR_ALREADY_APPLIED: combo already applied
```

---

## Backwards Compatibility

The fluent API coexists with the function-based API:

```lua
-- Old function-based API (still works)
set_binding("base", 1, "&kp Q")
upsert_combo("esc", {1, 2}, "&kp ESC", 50, {}, {})
set_layer_metadata("base", {["display-name"] = "Base"})

-- New fluent API
local esc = layout:combo("esc"):keys({1, 2}):binding("&kp ESC"):timeout(50)
layout:layer("base")
    :bind(1, "&kp Q")
    :meta("display-name", "Base")
    :apply()

-- Mix both in same script
set_binding("base", 1, "&kp W")
layout:layer("base"):bind(2, esc):apply()
```

---

## Testing Strategy

### Unit Tests
- Builder lifecycle (staged→applied errors, double-apply, auto-apply when used as binding)
- Index/type validation and 1-based conversions
- Delete/move helpers with dependent cleanup/reindex
- Serialization helpers (DTS/JSON string+file, template overrides, canonical ordering)
- Query snapshot immutability and builders seeded from existing definitions

### Integration Tests
- Lua scripts covering create/edit/delete across layers/combos/behaviors/macros/inputs/conditionals
- Round-trip DTS/JSON via load/parse/save + template render helpers
- CLI `script` command exercising old + fluent APIs together
- Deterministic error messages for invalid types/indices/lifecycle misuse

### Test Fixtures
- Create from scratch and edit existing layouts (seeded builders)
- Delete/move with dependent cleanup
- Template-based JSON↔DTS conversions
- Mixed function/fluent API flows
- Error-handling scenarios (invalid indices/types/double-apply)

---

## Success Criteria

- [ ] Builders apply real mutations for layers/combos/behaviors/macros/inputs/conditionals (no placeholders)
- [ ] Staged→applied lifecycle enforced everywhere with centralized 1-based validation and clear Lua errors
- [ ] Layout-level helpers (meta/delete/move) plus DTS/JSON string+file I/O with template support and canonical ordering
- [ ] Query/list coverage for all constructs with metadata/properties and read-only snapshots
- [ ] Function-based API remains compatible when the fluent API is registered via `tasks::lua_engine`
- [ ] Docs/examples/CHANGELOG entries land with each phase alongside regression fixtures

---

## Implementation Timeline

Each phase ships with tests + CHANGELOG entry:

1. Harden builders & lifecycle
2. Layout helpers & serialization
3. Query expansion
4. Integration, fixtures, docs

Total: **4 commits** with CHANGELOG.md updated for each phase.
