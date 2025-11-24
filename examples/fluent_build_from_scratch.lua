-- Fluent API example: generate a complete keymap from scratch, then hand the result to the
-- firmware builder. Run:
--   zmk-layout script \
--     --script examples/fluent_build_from_scratch.lua \
--     --layout /dev/null \
--     --output config/keymap.generated.dts
--   zmk-layout firmware build \
--     --manifest profiles/firmwares/glove80.toml \
--     --keyboard glove80 \
--     --toolchain zmk \
--     --target left \
--     --layout-dts config/keymap.generated.dts \
--     --output dist/glove80-left

-- Seed a bare document so we are not dependent on an existing DTS.
layout:parse_dts([[
keymap {
    compatible = "zmk,keymap";
    base {
        bindings = < >;
    };
};
]])

-- Behaviors we will reuse across layers.
layout:behavior("hrm_left")
    :param("flavor", "hold-preferred")
    :param("tapping-term-ms", 180)
    :apply()

layout:behavior("hrm_right")
    :param("flavor", "hold-preferred")
    :param("tapping-term-ms", 180)
    :apply()

-- Combos for frequently used shortcuts.
local esc = layout:combo("esc_combo")
    :keys({1, 2})
    :binding("&kp ESC")
    :timeout(40)

local tab = layout:combo("tab_combo")
    :keys({3, 4})
    :binding("&kp TAB")
    :timeout(40)

-- Base layer using macros/combos/behaviors directly as bindings.
layout:layer("base")
    :bindings({
        "&kp Q", "&kp W", "&kp E", "&kp R", esc,
        "&kp A", "&kp S", "&kp D", "&kp F", "&kp G",
        "&kp Z", "&kp X", "&kp C", "&kp V", "&kp B",
        "&kp LSHIFT", "&kp LCTRL", "&kp LALT", "&kp LGUI", tab,
    })
    :meta("display-name", "Base")
    :apply()

-- Navigation layer keeps layout order but swaps bindings.
layout:layer("nav")
    :bindings({
        "&kp HOME", "&kp UP", "&kp PGUP", "&none", "&none",
        "&kp LEFT", "&kp DOWN", "&kp RIGHT", "&kp DEL", "&none",
        "&kp END", "&kp PGDN", "&kp INS", "&none", "&none",
        "&mo base", "&kp LCTRL", "&kp LALT", "&kp LGUI", "&none",
    })
    :meta("display-name", "Nav")
    :apply()

-- Simple macro and reuse it in both layers.
local shrug = layout:macro("shrug")
    :tap("LSHIFT")
    :tap("9")
    :tap("0")
    :tap("SPACE")

layout:layer("base"):bind(20, shrug):apply()
layout:layer("nav"):bind(20, shrug):apply()

log("Generated keymap with layers: " .. table.concat(layout:list_layers(), ", "))
