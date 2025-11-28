-- Recreate the TailorKey sample keymap by reading the JSON payload and
-- emitting a fresh DTS/KEYMAP via the fluent API (layers, combos, macros,
-- hold-taps). This keeps the data in script form instead of relying on the
-- built-in JSON import shortcut.
--
-- Run:
--   zmk-layout keymap lua \
--     --script examples/recreate_tailorkey.lua \
--     --layout /dev/null \
--     --output out/tailorkey_from_json.keymap

-- Minimal JSON loader (prefers cjson, then dkjson).
local function load_json_module()
    local ok, mod = pcall(require, "cjson.safe")
    if ok and mod then
        return { decode = mod.decode, encode = mod.encode }
    end
    ok, mod = pcall(require, "cjson")
    if ok and mod then
        return { decode = mod.decode, encode = mod.encode }
    end
    ok, mod = pcall(require, "dkjson")
    if ok and mod then
        return {
            decode = function(str)
                local res, _, err = mod.decode(str)
                if err then
                    error(err)
                end
                return res
            end,
            encode = function(tbl)
                return mod.encode(tbl)
            end,
        }
    end
    error("No JSON module found (tried cjson.safe, cjson, dkjson)")
end

local json = load_json_module()

local sample_json = "examples/samples/8e349bac-1664-41f1-8d2e-7b9398f6d8cc_TailorKey v4.2i Bilateral.json"
local output_path = "out/tailorkey_from_json.keymap"

-- Helpers ------------------------------------------------------------------
local function read_file(path)
    local f = assert(io.open(path, "r"))
    local content = f:read("*a")
    f:close()
    return content
end

local function trim_binding_name(name)
    if type(name) == "string" and name:sub(1, 1) == "&" then
        return name:sub(2)
    end
    return name
end

local function render_binding(node)
    if type(node) == "string" then
        return node
    end
    if type(node) == "table" then
        local value = node.value or error("binding node missing value")
        local params = node.params or {}
        if #params == 0 then
            return value
        end
        local parts = {}
        for i, child in ipairs(params) do
            parts[i] = render_binding(child)
        end
        return value .. " " .. table.concat(parts, " ")
    end
    error("unsupported binding node type: " .. type(node))
end

local function map_list(list, fn)
    local out = {}
    for i, item in ipairs(list or {}) do
        out[i] = fn(item)
    end
    return out
end

local function format_num(n)
    return string.format("< %s >", n)
end

local function format_num_list(list)
    if not list or #list == 0 then
        return nil
    end
    return "< " .. table.concat(list, " ") .. " >"
end

local function to_standard_layout(data)
    local layers = {}
    for idx, layer_name in ipairs(data.layer_names or {}) do
        table.insert(layers, {
            name = layer_name,
            bindings = map_list(data.layers[idx] or {}, render_binding),
        })
    end

    local macros = {}
    for _, macro in ipairs(data.macros or {}) do
        local cells = macro.params and #macro.params or 0
        table.insert(macros, {
            name = trim_binding_name(macro.name),
            description = macro.description or "",
            bindings = map_list(macro.bindings or {}, render_binding),
            wait_ms = macro.waitMs,
            tap_ms = macro.tapMs,
            binding_cells = cells > 0 and cells or nil,
            compatible = cells > 0 and "zmk,behavior-macro-one-param" or "zmk,behavior-macro",
        })
    end

    local behaviors = {}
    for _, ht in ipairs(data.holdTaps or {}) do
        local props = {}
        if ht.tappingTermMs then
            props["tapping-term-ms"] = format_num(ht.tappingTermMs)
        end
        if ht.quickTapMs then
            props["quick-tap-ms"] = format_num(ht.quickTapMs)
        end
        if ht.requirePriorIdleMs then
            props["require-prior-idle-ms"] = format_num(ht.requirePriorIdleMs)
        end
        if ht.holdTriggerKeyPositions and #ht.holdTriggerKeyPositions > 0 then
            props["hold-trigger-key-positions"] = format_num_list(ht.holdTriggerKeyPositions)
        end
        if ht.holdTriggerOnRelease ~= nil then
            props["hold-trigger-on-release"] = ht.holdTriggerOnRelease and "true" or "false"
        end
        if ht.flavor then
            props["flavor"] = string.format("\"%s\"", ht.flavor)
        end

        table.insert(behaviors, {
            name = trim_binding_name(ht.name),
            description = ht.description or "",
            compatible = "zmk,behavior-hold-tap",
            binding_cells = 2,
            bindings = map_list(ht.bindings or {}, render_binding),
            properties = props,
        })
    end

    local combos = {}
    for _, combo in ipairs(data.combos or {}) do
        table.insert(combos, {
            name = combo.name,
            description = combo.description or "",
            key_positions = combo.keyPositions or {},
            binding = render_binding(combo.binding),
            timeout_ms = combo.timeoutMs,
            layers = combo.layers or {},
        })
    end

    local input_listeners = {}
    for _, listener in ipairs(data.inputListeners or {}) do
        local nodes = {}
        for _, node in ipairs(listener.nodes or {}) do
            local node_procs = map_list(node.inputProcessors or {}, function(proc)
                return { code = proc.code, params = proc.params or {} }
            end)
            table.insert(nodes, {
                code = node.code,
                description = node.description,
                layers = node.layers or {},
                inputProcessors = node_procs,
            })
        end
        table.insert(input_listeners, {
            code = listener.code,
            inputProcessors = listener.inputProcessors or {},
            nodes = nodes,
        })
    end

    local metadata = {
        title = data.title,
        author = data.creator,
        description = data.notes,
        extras = {
            keyboard = data.keyboard,
            uuid = data.uuid,
            parent_uuid = data.parent_uuid,
            tags = data.tags or {},
        },
    }

    return {
        layers = layers,
        combos = combos,
        behaviors = behaviors,
        macros = macros,
        input_listeners = input_listeners,
        metadata = metadata,
    }
end

-- Build layout -------------------------------------------------------------
local data = json.decode(read_file(sample_json))
local standard = to_standard_layout(data)
local standard_json = assert(json.encode(standard))

layout:parse_json(standard_json, "examples/moergo_glove80.j2")
layout:save_dts(output_path)

log(string.format(
    "Recreated TailorKey layout: %d layers, %d macros, %d hold-taps, %d combos -> %s",
    #(standard.layers or {}),
    #(data.macros or {}),
    #(data.holdTaps or {}),
    #(data.combos or {}),
    output_path
))
