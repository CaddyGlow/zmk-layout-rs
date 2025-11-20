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

## Implementation Phases

### Phase 1: Architecture & Core Objects (ComboObject + LayerBuilder)

**Commit 1**: Core fluent API with combo and layer support

- Create `src/lua_api/` module structure
- Implement `ComboObject` with fluent methods
- Implement `LayerBuilder` with mixed-type binding support
- Implement `LayoutAPI` entry point
- Add comprehensive unit tests
- Update CHANGELOG.md

**Files**:
- `src/lua_api/mod.rs`
- `src/lua_api/combo.rs`
- `src/lua_api/layer.rs`
- `src/lua_api/api.rs`
- `src/lib.rs`
- `tests/lua_api.rs`

---

### Phase 2: BehaviorObject

**Commit 2**: Add behavior configuration support

- Implement `BehaviorObject` with param() and bindings() methods
- Add behavior query methods
- Wire into LayoutAPI
- Add tests
- Update CHANGELOG.md

**Files**:
- `src/lua_api/behavior.rs`
- `src/lua_api/api.rs` (update)
- `tests/lua_api.rs` (update)

---

### Phase 3: MacroObject

**Commit 3**: Add macro definition support

- Implement `MacroObject` with action builders
- Add press/release/tap/wait methods
- Support multi-key actions
- Auto-apply on first use as binding
- Add tests
- Update CHANGELOG.md

**Files**:
- `src/lua_api/macro_builder.rs`
- `src/lua_api/api.rs` (update)
- `tests/lua_api.rs` (update)

---

### Phase 4: InputObject & ConditionalObject

**Commit 4**: Add input listener and conditional layer support

- Implement `InputObject` for encoders/sensors
- Implement `ConditionalObject` for conditional layers
- Add respective tests
- Update CHANGELOG.md

**Files**:
- `src/lua_api/input.rs`
- `src/lua_api/conditional.rs`
- `src/lua_api/api.rs` (update)
- `tests/lua_api.rs` (update)

---

### Phase 5: Query API

**Commit 5**: Add read-only query API for layout inspection

- Implement read-only info objects (LayerInfo, ComboInfo, etc.)
- Add query methods to LayoutAPI
- Add list_layers(), list_combos(), list_behaviors()
- Enable get_* methods on builder objects
- Add tests
- Update CHANGELOG.md

**Files**:
- `src/lua_api/query.rs`
- `src/lua_api/combo.rs` (add get_* methods)
- `src/lua_api/layer.rs` (add get_* methods)
- `src/lua_api/behavior.rs` (add get_* methods)
- `src/lua_api/api.rs` (update)
- `tests/lua_api.rs` (update)

---

### Phase 6: Integration

**Commit 6**: Integrate fluent API into script system

- Register LayoutAPI in `register_script_api()`
- Maintain backwards compatibility
- Add integration tests with real keymaps
- Create test fixtures
- Update CHANGELOG.md

**Files**:
- `src/tasks/mod.rs`
- `tests/lua_api.rs` (integration tests)
- `tests/fixtures/fluent_api_*.lua`

---

### Phase 7: Documentation

**Commit 7**: Complete API documentation and examples

- Create comprehensive API reference doc
- Write example scripts for all patterns
- Migration guide from function-based API
- Update README
- Update CHANGELOG.md

**Files**:
- `docs/fluent_lua_api.md`
- `examples/fluent_api_*.lua`
- `README.md`
- `docs/customization_tasks.md`

---

## Error Handling

All builder objects validate input and provide clear error messages:

```lua
-- Missing required fields
local combo = layout:combo("bad")
combo:keys({1, 2})  -- Missing binding
-- Error: combo 'bad' requires a binding before use

-- Invalid binding type
layout:layer("base"):bind(1, 123)
-- Error: binding must be string or object, got number

-- Index out of range
layout:layer("base"):bind(0, "&kp Q")
-- Error: binding index must be >= 1 (Lua 1-based)

-- Double apply on immutable object
local combo = layout:combo("esc"):keys({1,2}):binding("&kp ESC")
layout:layer("base"):bind(1, combo):apply()  -- Auto-applies combo
combo:timeout(50):apply()  -- Error: combo already applied
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
- Individual builder object creation
- Method chaining
- Type validation
- Error cases
- Lazy evaluation

### Integration Tests
- Load real keymap fixtures
- Execute Lua scripts using fluent API
- Verify document mutations
- Test query API accuracy
- Test edit operations

### Test Fixtures
- Create from scratch
- Edit existing layout
- Mixed function/fluent API
- Error handling scenarios
- Query operations

---

## Success Criteria

- [ ] All 7 phases committed with CHANGELOG entries
- [ ] 100% test coverage for new code
- [ ] All ZMK constructs supported
- [ ] Query API for reading layout state
- [ ] Edit existing layouts, not just create new
- [ ] Backwards compatibility maintained
- [ ] Documentation complete with examples
- [ ] No breaking changes

---

## Implementation Timeline

Each phase is one commit:

1. Core Objects (Combo + Layer) - **Commit 1**
2. BehaviorObject - **Commit 2**
3. MacroObject - **Commit 3**
4. InputObject + ConditionalObject - **Commit 4**
5. Query API - **Commit 5**
6. Integration - **Commit 6**
7. Documentation - **Commit 7**

Total: **7 commits** with CHANGELOG.md updated for each phase.
