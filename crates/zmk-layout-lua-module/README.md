# zmk-layout-lua-module

Native Lua module for ZMK keyboard layout manipulation. This builds a shared library (`.so`/`.dll`/`.dylib`) that can be loaded by any Lua 5.4 interpreter.

## Building

This crate is excluded from the main workspace due to mlua feature conflicts. Build it separately:

```bash
cd crates/zmk-layout-lua-module
cargo build --release
```

Output: `target/release/libzmk_layout.so` (Linux), `libzmk_layout.dylib` (macOS), or `zmk_layout.dll` (Windows)

## Installation

Copy the library to your Lua module path and rename it:

```bash
cp target/release/libzmk_layout.so /usr/local/lib/lua/5.4/zmk_layout.so
# Or for local use:
cp target/release/libzmk_layout.so ./zmk_layout.so
```

## Usage

```lua
local zmk = require("zmk_layout")

-- Check version
print(zmk.version)  -- "0.1.0"

-- Create a new layout
local layout = zmk.layout()

-- Parse DTS content
layout:parse_dts([[
/ {
    keymap {
        compatible = "zmk,keymap";
        default_layer {
            bindings = <&kp A &kp B &kp C>;
        };
    };
};
]])

-- List layers
local layers = layout:list_layers()
print("Layers: " .. table.concat(layers, ", "))

-- Modify a layer
layout:layer("default_layer")
    :bindings({"&kp Q", "&kp W", "&kp E"})
    :apply()

-- Add a combo
layout:combo("esc_combo")
    :keys({1, 2})
    :binding("&kp ESC")
    :timeout(50)
    :apply()

-- Export
print(layout:to_dts_string())
layout:save_dts("output.keymap")
```

## API Reference

### Module Functions

- `zmk.layout()` - Create a new layout object
- `zmk.version` - Module version string
- `zmk.hello(name)` - Test function

### Layout Object Methods

**Layer Management:**
- `layout:layer(name)` - Get layer builder
- `layout:list_layers()` - List all layer names
- `layout:move_layer(name, index)` - Reorder a layer
- `layout:remove_layer(name)` - Delete a layer
- `layout:get_layer(name)` - Query layer info

**Combos:**
- `layout:combo(name)` - Get combo builder
- `layout:list_combos()` - List all combo names
- `layout:get_combo(name)` - Query combo info

**Behaviors:**
- `layout:behavior(name)` - Get behavior builder
- `layout:list_behaviors()` - List all behavior names
- `layout:get_behavior(name)` - Query behavior info

**Macros:**
- `layout:macro(name)` - Get macro builder

**I/O:**
- `layout:load_dts(path)` - Load from .keymap file
- `layout:parse_dts(source)` - Parse DTS string
- `layout:save_dts(path)` - Save to .keymap file
- `layout:to_dts_string()` - Export as DTS string
- `layout:load_json(json_path, template_path)` - Load JSON layout
- `layout:save_json(path)` - Save as JSON

**Other:**
- `layout:new()` - Clear the layout
- `layout:meta(key, value)` - Set metadata

### Builder Objects

All builders use a fluent API with `:apply()` to commit changes.

**LayerBuilder:**
```lua
layout:layer("name")
    :bindings({"&kp A", "&kp B"})
    :meta("key", "value")
    :apply()
```

**ComboObject:**
```lua
layout:combo("name")
    :keys({1, 2, 3})        -- 1-based key positions
    :binding("&kp ESC")
    :timeout(50)            -- milliseconds
    :on_layers({"default"}) -- optional layer restriction
    :apply()
```

**BehaviorObject:**
```lua
layout:behavior("name")
    :param("flavor", "tap-preferred")
    :param("tapping-term-ms", 200)
    :bindings({"&kp", "&kp"})
    :apply()
```

**MacroObject:**
```lua
layout:macro("name")
    :tap("A")
    :wait(100)
    :tap("B")
    :apply()
```

## Requirements

- Lua 5.4 interpreter
- The module links against the system Lua library (not vendored)

## License

Same as the parent zmk-layout-rs project.
