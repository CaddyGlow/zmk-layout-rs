-- Demonstrates the Lua I/O helpers (DTS/JSON/template) plus a dry-run firmware build.
-- Usage:
--   zmk-layout script \
--     --script examples/fluent_io_and_build.lua \
--     --layout examples/samples/Factory.keymap \
--     --output out/keymap.generated.dts \
--     --diff

-- Load an existing DTS (or .keymap rendered DTS) and normalize it.
layout:load_dts("examples/samples/Factory.keymap")

-- Export to JSON without a template.
local json = layout:to_json_string()
layout:save_json("out/layout.json")

-- Re-import via template-aware JSON to show symmetry.
local template_path = "examples/moergo_glove80.j2"
local rendered = layout:render_template(json, template_path)
layout:parse_dts(rendered)

-- Save updated DTS.
layout:save_dts("out/keymap.generated.dts")

-- Kick off a firmware build as a dry-run (no Docker work is performed).
local build = layout:build_firmware({
    manifest = "profiles/firmwares/glove80.toml",
    keyboard = "glove80",
    layout_dts = "out/keymap.generated.dts",
    targets = {"left"},
    output_dir = "out/firmware",
    dry_run = true,
    disable_cache = true,
})

log("dry-run build request:")
log("  keyboard = " .. build.request.keyboard)
log("  targets  = " .. table.concat(build.request.targets, ", "))
