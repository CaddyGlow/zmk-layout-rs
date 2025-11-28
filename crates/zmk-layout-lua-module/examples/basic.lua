-- Basic example of using the zmk_layout native Lua module
--
-- Build the module first:
--   cd crates/zmk-layout-lua-module
--   cargo build --release
--
-- Then copy to your Lua path:
--   cp target/release/libzmk_layout.so ./zmk_layout.so
--
-- Run this example:
--   lua examples/basic.lua

local zmk = require("zmk_layout")

-- Check version
print("ZMK Layout Module v" .. zmk.version)
print(zmk.hello("user"))
print()

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
    :bindings({"&kp Q", "&kp W", "&kp E", "&kp R", "&kp T"})
    :apply()

-- Add a combo
layout:combo("esc_combo")
    :keys({1, 2})
    :binding("&kp ESC")
    :timeout(50)
    :apply()

-- Add a macro
layout:macro("greeting")
    :tap("H")
    :tap("I")
    :apply()

-- Output the result
print()
print("Generated DTS:")
print("==============")
print(layout:to_dts_string())
