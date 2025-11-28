# zmk-layout-python

Python bindings for ZMK layout manipulation.

## Building

This crate uses [PyO3](https://pyo3.rs/) to create a native Python extension module. The recommended way to build and install is using [maturin](https://github.com/PyO3/maturin):

```bash
# Install maturin
pip install maturin

# Build and install in development mode
cd crates/zmk-layout-python
maturin develop

# Or build a wheel
maturin build --release
```

Alternatively, you can use cargo directly to build the shared library:

```bash
cargo build --release
```

This produces `target/release/libzmk_layout.so` (Linux), `libzmk_layout.dylib` (macOS), or `zmk_layout.dll` (Windows). You'll need to rename and place this file appropriately for Python to import it.

## Usage

```python
import zmk_layout

# Create a new layout
layout = zmk_layout.Layout()

# Load from file
layout.load_dts("path/to/keymap.keymap")

# Or parse inline
layout.parse_dts('''
    / {
        keymap {
            compatible = "zmk,keymap";
            default_layer { bindings = <&kp A>; };
        };
    };
''')

# Modify layers using fluent API
layout.layer("default").bindings(["&kp A", "&kp B", "&kp C"]).apply()

# Set individual bindings (1-based indexing)
layout.layer("default").bind(1, "&kp Q").bind(2, "&kp W").apply()

# Add combos (1-based key positions)
layout.combo("esc").keys([1, 2]).binding("&kp ESC").timeout(50).apply()

# Add behaviors
layout.behavior("my_lt").bindings(["&mo", "&kp"]).apply()

# Configure macros
layout.macro_("my_macro").tap("A").wait(100).tap("B").apply()

# Configure encoders/inputs
layout.input("encoder").on_turn_cw("&kp UP").on_turn_ccw("&kp DOWN").apply()

# Configure conditionals
layout.conditional("caps_word").condition("caps_word_active").then_layer("caps").apply()

# Query layout info
layers = layout.list_layers()  # ["base", "nav", "num"]
layer_info = layout.get_layer("base")
if layer_info:
    print(f"Layer {layer_info.name} has {len(layer_info.bindings())} bindings")

# Export
layout.save_dts("output.keymap")
dts_string = layout.to_dts_string()
json_string = layout.to_json_string()

# Build firmware
result = layout.build_firmware(
    manifest="glove80.toml",
    keyboard="glove80",
    use_current=True,
    output_dir="out/firmware"
)
if result["success"]:
    print(f"Firmware built: {result['artifacts']}")
```

## API Reference

### Layout

Main class for manipulating ZMK keyboard layouts.

**Methods:**
- `layer(name)` - Get a LayerBuilder for modifying a layer
- `combo(name)` - Get a ComboObject for configuring a combo
- `behavior(name)` - Get a BehaviorObject for configuring a behavior
- `macro_(name)` - Get a MacroObject for building a macro
- `input(name)` - Get an InputObject for configuring encoders/sensors
- `conditional(name)` - Get a ConditionalObject for conditional logic
- `clear()` - Reset to an empty layout
- `move_layer(name, index)` - Move a layer to a new position (1-based)
- `remove_layer(name)` - Remove a layer by name
- `meta(key, value)` - Set layout metadata
- `get_layer(name)` - Get LayerInfo or None
- `get_combo(name)` - Get ComboInfo or None
- `get_behavior(name)` - Get BehaviorInfo or None
- `list_layers()` - List all layer names
- `list_combos()` - List all combo names
- `list_behaviors()` - List all behavior names
- `load_dts(path)` / `load_dtsi(path)` - Load a DTS file
- `load_json(json_path, template_path)` - Load JSON with template
- `save_dts(path)` / `save_dtsi(path)` - Save as DTS
- `save_json(path)` - Save as JSON
- `parse_dts(source)` - Parse DTS from string
- `parse_json(json, template_path)` - Parse JSON with template
- `to_dts_string()` - Export as DTS string
- `to_json_string()` - Export as JSON string
- `render_template(json, template_path)` - Render JSON with template
- `build_firmware(...)` - Build firmware with current layout

### LayerBuilder

Fluent API for modifying layer bindings.

**Methods:**
- `bind(index, binding)` - Set binding at index (1-based)
- `bindings(list)` - Replace all bindings
- `meta(key, value)` - Set layer metadata
- `get_binding(index)` - Get binding at index
- `get_bindings()` - Get all bindings
- `get_meta(key)` - Get metadata value
- `apply()` - Apply changes

### ComboObject

Fluent API for configuring combos.

**Methods:**
- `keys(positions)` - Set key positions (1-based)
- `binding(binding)` - Set the binding to execute
- `timeout(ms)` - Set timeout in milliseconds
- `on_layers(layers)` - Restrict to specific layers
- `when(condition)` - Add activation condition
- `get_keys()` / `get_binding()` / `get_timeout()` - Query values
- `apply()` - Apply changes

### BehaviorObject

Fluent API for configuring behaviors.

**Methods:**
- `param(key, value)` - Set a parameter
- `bindings(list)` - Set bindings
- `get_param(key)` - Get parameter value
- `get_bindings()` - Get bindings
- `apply()` - Apply changes

### MacroObject

Fluent API for building macros.

**Methods:**
- `press(keys)` - Add press action
- `release(keys)` - Add release action
- `tap(keys)` - Add tap action
- `wait(ms)` - Add delay
- `wait_release()` - Wait for key release
- `wait_tap()` - Wait for tap
- `get_actions()` - Get all actions
- `apply()` - Apply changes

### InputObject

Fluent API for encoder/sensor configuration.

**Methods:**
- `type_(value)` - Set input type
- `on_turn_cw(binding)` - Set clockwise binding
- `on_turn_ccw(binding)` - Set counter-clockwise binding
- `on_press(binding)` - Set press binding
- `resolution(value)` - Set resolution
- `get_type()` / `get_cw_binding()` / `get_ccw_binding()` - Query values
- `apply()` - Apply changes

### ConditionalObject

Fluent API for conditional layer logic.

**Methods:**
- `condition(expr)` - Set condition expression
- `then_layer(layer)` - Set layer when true
- `else_layer(layer)` - Set layer when false
- `get_condition()` / `get_then_layer()` / `get_else_layer()` - Query values
- `apply()` - Apply changes

### Query Objects

Read-only information objects:

- **LayerInfo**: `name`, `bindings()`, `metadata()`
- **ComboInfo**: `name`, `keys()`, `binding()`, `timeout()`
- **BehaviorInfo**: `name`, `bindings()`, `get(key)`
