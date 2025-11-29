-- Recreate the TailorKey sample keymap from code (no external JSON reads).
-- Data lives in small modules and we build the standard layout table, encode
-- it to JSON, and render via the Glove80 template.
--
-- Run:
--   zmk-layout keymap lua \
--     --script examples/recreate_tailorkey.lua \
--     --layout /dev/null \
--     --output out/tailorkey_from_json.keymap

local layers_data = dofile("examples/tailorkey_layers.lua")
local behaviors_data = dofile("examples/tailorkey_behaviors.lua")
local combos_data = dofile("examples/tailorkey_combos.lua")

-- JSON encoder (minimal, handles tables, strings, numbers, bools, nil).
local function escape(str)
    return (str:gsub("\\", "\\\\")
        :gsub("\"", "\\\"")
        :gsub("\b", "\\b")
        :gsub("\f", "\\f")
        :gsub("\n", "\\n")
        :gsub("\r", "\\r")
        :gsub("\t", "\\t"))
end

local function is_array(tbl)
    local max_idx = 0
    local count = 0
    for k, _ in pairs(tbl) do
        if type(k) ~= "number" then
            return false
        end
        if k > max_idx then
            max_idx = k
        end
        count = count + 1
    end
    if max_idx == 0 then
        return true, 0
    end
    for i = 1, max_idx do
        if tbl[i] == nil then
            return false
        end
    end
    return true, max_idx
end

local function encode_json(value)
    local t = type(value)
    if t == "string" then
        return "\"" .. escape(value) .. "\""
    elseif t == "number" or t == "boolean" then
        return tostring(value)
    elseif t == "nil" then
        return "null"
    elseif t == "table" then
        local array, max_idx = is_array(value)
        if array then
            local parts = {}
            for i = 1, max_idx do
                parts[i] = encode_json(value[i])
            end
            return "[" .. table.concat(parts, ",") .. "]"
        else
            local parts = {}
            for k, v in pairs(value) do
                parts[#parts + 1] = "\"" .. escape(k) .. "\":" .. encode_json(v)
            end
            return "{" .. table.concat(parts, ",") .. "}"
        end
    else
        error("unsupported json type: " .. t)
    end
end

local function build_layers()
    local built = {}
    local base = layers_data.base
    built[1] = { name = layers_data.layer_names[1], bindings = base }
    for i = 2, #layers_data.layer_names do
        local name = layers_data.layer_names[i]
        local overrides = layers_data.overrides[name] or {}
        local bindings = {}
        for idx = 1, #base do
            bindings[idx] = "&trans"
        end
        for idx, val in pairs(overrides) do
            bindings[idx] = val
        end
        built[#built + 1] = { name = name, bindings = bindings }
    end
    return built
end

local function build_macros()
    local result = {}
    for _, macro in ipairs(behaviors_data.macros) do
        local cells = macro.params and #macro.params or 0
        result[#result + 1] = {
            name = macro.name,
            description = macro.description or "",
            bindings = macro.bindings,
            wait_ms = macro.wait_ms,
            tap_ms = macro.tap_ms,
            binding_cells = cells > 0 and cells or nil,
            compatible = cells > 0 and "zmk,behavior-macro-one-param" or "zmk,behavior-macro",
        }
    end
    return result
end

local function format_property(value)
    if value == nil then
        return nil
    end
    local t = type(value)
    if t == "number" then
        return string.format("< %s >", value)
    elseif t == "boolean" then
        return value and "true" or "false"
    elseif t == "table" then
        local parts = {}
        for i, v in ipairs(value) do
            parts[i] = tostring(v)
        end
        return "< " .. table.concat(parts, " ") .. " >"
    elseif t == "string" then
        return string.format("\"%s\"", value)
    end
    return tostring(value)
end

local function build_behaviors()
    local result = {}
    for _, ht in ipairs(behaviors_data.hold_taps) do
        local props = {}
        props["tapping-term-ms"] = format_property(ht.tapping_term_ms)
        props["quick-tap-ms"] = format_property(ht.quick_tap_ms)
        props["require-prior-idle-ms"] = format_property(ht.require_prior_idle_ms)
        if ht.hold_trigger_key_positions and #ht.hold_trigger_key_positions > 0 then
            props["hold-trigger-key-positions"] = format_property(ht.hold_trigger_key_positions)
        end
        if ht.hold_trigger_on_release ~= nil then
            props["hold-trigger-on-release"] = format_property(ht.hold_trigger_on_release)
        end
        if ht.flavor then
            props["flavor"] = format_property(ht.flavor)
        end
        result[#result + 1] = {
            name = ht.name,
            description = ht.description or "",
            compatible = "zmk,behavior-hold-tap",
            binding_cells = 2,
            bindings = ht.bindings,
            properties = props,
        }
    end
    return result
end

local function build_standard_layout()
    return {
        layers = build_layers(),
        combos = combos_data.combos,
        behaviors = build_behaviors(),
        macros = build_macros(),
        input_listeners = combos_data.input_listeners,
        metadata = combos_data.metadata,
    }
end

local function save_keymap()
    local standard_layout = build_standard_layout()
    local json = encode_json(standard_layout)
    local output_path = "out/tailorkey_from_json.keymap"

    os.execute("mkdir -p out")
    layout:parse_json(json, "examples/moergo_glove80.j2")
    layout:save_dts(output_path)
    log(string.format("Recreated TailorKey layout -> %s", output_path))
end

save_keymap()
