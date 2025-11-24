-- Fluent API example: patch an existing keymap in-place. Run:
--   zmk-layout script \
--     --script examples/fluent_patch_existing.lua \
--     --layout config/keymap.dts \
--     --output config/keymap.generated.dts \
--     --diff

-- Make sure the base layer exists.
local base_info = layout:get_layer("base")
if not base_info then
    error("expected a 'base' layer in the provided layout")
end

-- Fill every transparent slot in base with space.
local base = layout:layer("base")
for idx = 1, #base_info:bindings() do
    if base:get_binding(idx) == "&trans" then
        base:bind(idx, "&kp SPACE")
    end
end
base:meta("display-name", "Base (patched)")
    :apply()

-- Add an encoder input if it does not exist yet.
layout:input("encoder_vol")
    :type("encoder")
    :on_turn_cw("&kp C_VOL_UP")
    :on_turn_ccw("&kp C_VOL_DN")
    :on_press("&kp C_MUTE")
    :resolution(2)
    :apply()

-- Tighten up an existing combo timeout or create the combo if missing.
layout:combo("escape_combo")
    :keys({1, 2})
    :binding("&kp ESC")
    :timeout(30)
    :apply()

log("Patched base layer and ensured encoder + escape combo exist")
