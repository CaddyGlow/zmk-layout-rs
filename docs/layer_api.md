# Layer Management API Documentation

This document describes the layer management functions available in Rhai scripts for ZMK keymap manipulation.

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
```rhai
// Add a new layer with basic bindings
add_layer("CustomNav", [
    "&kp LEFT",
    "&kp DOWN",
    "&kp UP",
    "&kp RIGHT"
]);
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
```rhai
// Remove a layer by name
remove_layer("Typing");

// With error handling
try {
    remove_layer("OldLayer");
    log("Layer removed successfully");
} catch (error) {
    log(`Failed to remove layer: ${error}`);
}
```

**Error Cases:**
- Layer doesn't exist
- Attempting to remove a system-critical layer

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
```rhai
let cursor = get_layer("Cursor");
log(`Layer: ${cursor.name}`);
log(`Index: ${cursor.index}`);
log(`Bindings: ${cursor.binding_count}`);

// Access specific binding
log(`First binding: ${cursor.bindings[0]}`);

// Iterate over bindings
for binding in cursor.bindings {
    log(`  ${binding}`);
}
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
```rhai
let layers = list_layers();

log(`Total layers: ${layers.len()}`);

for layer in layers {
    log(`${layer.index}. ${layer.name} (${layer.binding_count} bindings)`);
}

// Filter for specific layers
let nav_layers = layers.filter(|l| l.name.contains("Nav"));
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
```rhai
let count = layer_count();
log(`Keymap has ${count} layers`);

// Check before adding
if layer_count() < 20 {
    add_layer("NewLayer", ["&kp A"]);
}
```

---

## Complete Example

Here's a complete script demonstrating layer management:

```rhai
log("=== Layer Management Example ===");

// 1. List current layers
log("\nCurrent layers:");
for layer in list_layers() {
    log(`  ${layer.index}. ${layer.name}`);
}

// 2. Get specific layer info
let cursor = get_layer("Cursor");
log(`\nCursor layer has ${cursor.binding_count} bindings`);

// 3. Remove unwanted layer
try {
    remove_layer("Typing");
    log("\nRemoved Typing layer");
} catch (error) {
    log(`\nCould not remove Typing layer: ${error}`);
}

// 4. Add new layer
add_layer("CustomFunc", [
    "&kp F1", "&kp F2", "&kp F3", "&kp F4",
    "&kp F5", "&kp F6", "&kp F7", "&kp F8"
]);
log("\nAdded CustomFunc layer");

// 5. Verify changes
log(`\nFinal layer count: ${layer_count()}`);
```

## Best Practices

### 1. Error Handling

Always use `try-catch` blocks when removing layers:

```rhai
try {
    remove_layer("OldLayer");
} catch (error) {
    log(`Warning: ${error}`);
}
```

### 2. Validation

Check if a layer exists before operations:

```rhai
try {
    let layer = get_layer("Target");
    // Layer exists, proceed
} catch {
    // Layer doesn't exist, handle accordingly
}
```

### 3. Naming Conventions

Follow these naming conventions for layers:
- Use `snake_case` or `PascalCase`: `layer_Name` or `LayerName`
- Avoid special characters except underscore
- Use descriptive names: `Navigation`, `Symbols`, `Function_Keys`

### 4. Layer Ordering

Remember that layer indices change when layers are added/removed:

```rhai
// Get current state before modifications
let initial_layers = list_layers();

// Perform modifications
remove_layer("OldLayer");
add_layer("NewLayer", bindings);

// Verify new state
let final_layers = list_layers();
log(`Layers changed from ${initial_layers.len()} to ${final_layers.len()}`);
```

## Integration with Existing Functions

These layer management functions work seamlessly with existing binding functions:

```rhai
// Get layer
let nav = get_layer("Navigation");

// Modify specific binding
set_binding("Navigation", 0, "&kp HOME");

// Update entire layer
set_layer("Navigation", ["&kp HOME", "&kp END", "&kp PG_UP", "&kp PG_DN"]);

// Reorder layers
// (assuming reorder_layer function exists)
reorder_layer("Navigation", 2);
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

All layer management functions return `Result` types in Rust and can throw exceptions in Rhai:

| Error | Cause | Rhai Behavior |
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
```rhai
// Manual manipulation (fragile)
log("You need to manually remove layer_Typing");
```

**After:**
```rhai
// Use the API (robust)
remove_layer("Typing");
```

## See Also

- Binding management functions: `set_binding()`, `set_layer()`
- Layer reordering: `reorder_layer()`
- Combo management: `upsert_combo()`
