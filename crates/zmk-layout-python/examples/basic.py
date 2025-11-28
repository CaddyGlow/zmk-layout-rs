#!/usr/bin/env python3
"""Basic example demonstrating zmk_layout Python module usage."""

import zmk_layout

# Check version
print(f"zmk_layout version: {zmk_layout.__version__}")

# Test the hello function
print(zmk_layout.hello())
print(zmk_layout.hello("ZMK User"))

# Create a new layout
layout = zmk_layout.Layout()

# Parse a simple keymap inline
layout.parse_dts("""
/ {
    keymap {
        compatible = "zmk,keymap";
        default_layer {
            label = "Base";
            bindings = <&kp A &kp B &kp C &kp D>;
        };
        nav_layer {
            label = "Nav";
            bindings = <&kp LEFT &kp DOWN &kp UP &kp RIGHT>;
        };
    };
};
""")

# List layers
print("\nLayers in keymap:")
for layer_name in layout.list_layers():
    info = layout.get_layer(layer_name)
    if info:
        print(f"  - {info.name}: {info.bindings()}")

# Modify a layer using the fluent API
print("\nModifying default layer...")
layout.layer("default_layer").bind(1, "&kp Q").bind(2, "&kp W").apply()

# Check the changes
info = layout.get_layer("default_layer")
if info:
    print(f"Updated bindings: {info.bindings()}")

# Add a combo
print("\nAdding combo...")
layout.combo("esc_combo").keys([1, 2]).binding("&kp ESC").timeout(50).apply()

# Add a new layer
print("\nAdding new layer...")
layout.layer("num_layer").bindings([
    "&kp N1", "&kp N2", "&kp N3", "&kp N4"
]).meta("label", "Numbers").apply()

# List layers again
print("\nFinal layers:")
for layer_name in layout.list_layers():
    print(f"  - {layer_name}")

# Export to DTS string
print("\nGenerated DTS:")
print("=" * 40)
dts = layout.to_dts_string()
print(dts[:500] + "..." if len(dts) > 500 else dts)

print("\nDone!")
