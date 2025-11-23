# Layer Management API Documentation

This document describes the layer management functions available in Lua scripts for ZMK keymap manipulation.

## Overview

The layer management API provides functions to add, remove, get, and list layers in a ZMK keymap. These functions follow Rust best practices with clear, descriptive names and proper error handling.

## Functions

### add_layer

Add a new layer to the keymap.

**Signature:**
```rust
fn add_layer(name: &str, bindings: Array) -> Result<(), Error>
```

**Parameters:**
- `name`: The name of the new layer (must be unique)
- `bindings`: Array of binding strings (e.g., `["&kp A", "&kp B", "&kp C"]`)

**Example:**
```lua
-- Add a new layer with basic bindings
add_layer("CustomNav", {
    "&kp LEFT",
    "&kp DOWN",
    "&kp UP",
    "&kp RIGHT",
})
```

**Error Cases:**
- Layer already exists
- Empty bindings array
- Invalid binding syntax

---

### remove_layer

Remove an existing layer from the keymap.

**Signature:**
```rust
fn remove_layer(name: &str) -> Result<(), Error>
```

**Parameters:**
- `name`: The name of the layer to remove

**Example:**
```lua
-- Remove a layer by name
remove_layer("Typing")

-- With error handling
local ok, err = pcall(function()
    remove_layer("OldLayer")
    log("Layer removed successfully")
end)
if not ok then
    log(string.format("Failed to remove layer: %s", err))
end
```

**Error Cases:**
- Layer doesn't exist
- Attempting to remove a system-critical layer

---

### Adapter pipeline

If you want to hydrate layouts from JSON or rendered DTS (with template capture) before mutating them, use `adapters::pipeline::AdapterPipeline`:

```rust
use zmk_layout_rs::adapters::{AdapterPipeline, TemplateParseMode};

let layout = AdapterPipeline::from_dts_text(rendered_dts)
    .template_source(template_source)
    .template_mode(TemplateParseMode::StripPlaceholders)
    .load()?;

let layers = layout.layers;
```

### Flash fake backend (CLI)

For local testing of `zmk-layout firmware devices/flash` without hardware, set:

- `ZMK_FLASH_FAKE_BACKEND=1`
- `ZMK_FLASH_FAKE_MOUNTPOINT=/path/to/tmpdir`
- `ZMK_FLASH_FAKE_NAME=FAKE_DEVICE`
- `ZMK_FLASH_FAKE_SERIAL=GLV80-FAKE`
- `ZMK_FLASH_FAKE_VENDOR=DemoVendor`
- `ZMK_FLASH_FAKE_MODEL=DemoModel`
- `ZMK_FLASH_FAKE_FSTYPE=vfat`

---

### get_layer

Get information about a specific layer.

**Signature:**
```rust
fn get_layer(name: &str) -> Result<Map, Error>
```

**Parameters:**
- `name`: The name of the layer to query

**Returns:**
A map containing:
- `name` (string): Layer name
- `index` (int): Layer position in keymap (0-based)
- `binding_count` (int): Number of key bindings
- `bindings` (array): Array of binding strings

**Example:**
```lua
local cursor = get_layer("Cursor")
log(string.format("Layer: %s", cursor.name))
log(string.format("Index: %d", cursor.index))
log(string.format("Bindings: %d", cursor.binding_count))

-- Access specific binding (Lua arrays are 1-based)
log(string.format("First binding: %s", cursor.bindings[1]))

-- Iterate over bindings
for _, binding in ipairs(cursor.bindings) do
    log(string.format("  %s", binding))
end
```

**Error Cases:**
- Layer doesn't exist

---

### list_layers

List all layers in the keymap.

**Signature:**
```rust
fn list_layers() -> Result<Array, Error>
```

**Returns:**
Array of maps, each containing the same fields as `get_layer()`

**Example:**
```lua
local layers = list_layers()

log(string.format("Total layers: %d", #layers))

for _, layer in ipairs(layers) do
    log(string.format("%d. %s (%d bindings)", layer.index, layer.name, layer.binding_count))
end

-- Filter for specific layers
local nav_layers = {}
for _, layer in ipairs(layers) do
    if string.find(layer.name, "Nav") then
        table.insert(nav_layers, layer)
    end
end
```

---

### layer_count

Get the total number of layers.

**Signature:**
```rust
fn layer_count() -> int
```

**Returns:**
Integer count of layers in the keymap

**Example:**
```lua
local count = layer_count()
log(string.format("Keymap has %d layers", count))

-- Check before adding
if layer_count() < 20 then
    add_layer("NewLayer", {"&kp A"})
end
```

---

## Complete Example

Here's a complete script demonstrating layer management:

```lua
log("=== Layer Management Example ===")

-- 1. List current layers
log("\nCurrent layers:")
for _, layer in ipairs(list_layers()) do
    log(string.format("  %d. %s", layer.index, layer.name))
end

-- 2. Get specific layer info
local cursor = get_layer("Cursor")
log(string.format("\nCursor layer has %d bindings", cursor.binding_count))

-- 3. Remove unwanted layer
local removed, remove_err = pcall(function()
    remove_layer("Typing")
end)
if removed then
    log("\nRemoved Typing layer")
else
    log(string.format("\nCould not remove Typing layer: %s", remove_err))
end

-- 4. Add new layer
add_layer("CustomFunc", {
    "&kp F1", "&kp F2", "&kp F3", "&kp F4",
    "&kp F5", "&kp F6", "&kp F7", "&kp F8",
})
log("\nAdded CustomFunc layer")

-- 5. Verify changes
log(string.format("\nFinal layer count: %d", layer_count()))
```

## Best Practices

### 1. Error Handling

Always use `try-catch` blocks when removing layers:

```lua
local ok, err = pcall(function()
    remove_layer("OldLayer")
end)
if not ok then
    log(string.format("Warning: %s", err))
end
```

### 2. Validation

Check if a layer exists before operations:

```lua
local exists = pcall(function()
    get_layer("Target")
end)
if exists then
    -- Layer exists, proceed
else
    -- Layer doesn't exist, handle accordingly
end
```

### 3. Naming Conventions

Follow these naming conventions for layers:
- Use `snake_case` or `PascalCase`: `layer_Name` or `LayerName`
- Avoid special characters except underscore
- Use descriptive names: `Navigation`, `Symbols`, `Function_Keys`

### 4. Layer Ordering

Remember that layer indices change when layers are added/removed:

```lua
-- Get current state before modifications
local initial_layers = list_layers()

-- Perform modifications
remove_layer("OldLayer")
add_layer("NewLayer", bindings)

-- Verify new state
local final_layers = list_layers()
log(string.format("Layers changed from %d to %d", #initial_layers, #final_layers))
```

## Integration with Existing Functions

These layer management functions work seamlessly with existing binding functions:

```lua
-- Get layer
local nav = get_layer("Navigation")

-- Modify specific binding
set_binding("Navigation", 0, "&kp HOME")

-- Update entire layer
set_layer("Navigation", {"&kp HOME", "&kp END", "&kp PG_UP", "&kp PG_DN"})

-- Reorder layers
move_layer("Navigation", 2)
```

## Rust API Reference

### LayoutEngine Methods

```rust
impl LayoutEngine {
    /// Add a new layer
    pub fn add_layer(&mut self, name: &str, bindings: &[String])
        -> Result<(), LayoutEngineError>;

    /// Remove a layer
    pub fn remove_layer(&mut self, name: &str)
        -> Result<(), LayoutEngineError>;

    /// Get layer information
    pub fn get_layer(&self, name: &str)
        -> Option<LayerInfo>;

    /// List all layers
    pub fn list_layers(&self)
        -> Vec<LayerInfo>;
}

/// Layer information structure
pub struct LayerInfo {
    pub name: String,
    pub index: usize,
    pub binding_count: usize,
    pub bindings: Vec<String>,
}
```

## Error Handling

All layer management functions return `Result` types in Rust and surface as runtime errors in Lua:

| Error | Cause | Lua Behavior |
|-------|-------|---------------|
| `LayerNotFound` | Layer doesn't exist | Throws exception |
| `LayerAlreadyExists` | Duplicate layer name | Throws exception |
| `ValidationError` | Invalid bindings/empty array | Throws exception |
| `ProviderError` | Internal provider error | Throws exception |

## Performance Considerations

- `list_layers()` creates a snapshot of all layers - cache the result if calling multiple times
- `get_layer()` is efficient for single layer queries
- `layer_count()` is O(1) - just returns the count
- Adding/removing layers triggers layer index updates for defines

## Migration Guide

If you have existing scripts that manually manipulate layer nodes, update them to use these functions:

**Before:**
```lua
-- Manual manipulation (fragile)
log("You need to manually remove layer_Typing")
```

**After:**
```lua
-- Use the API (robust)
remove_layer("Typing")
```

## See Also

- Binding management functions: `set_binding()`, `set_layer()`
- Layer reordering: `move_layer()`
- Combo management: `upsert_combo()`
