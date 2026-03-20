-- Example script using the fluent layout API
-- See docs/layer_api.md for full documentation

-- Load key position names for Glove80
layout:load_positions("glove80")
local positions = layout:get_positions()

local layers = layout:list_layers()

for _, layer_name in ipairs(layers) do
	local define_name = layer_name
	if string.sub(define_name, 1, 6) == "layer_" then
		define_name = string.sub(define_name, 7)
	end
	if string.upper(define_name) == "QWERTY" then
		break
	end

	log(string.format("Removing layer: %s", layer_name))
	layout:remove_layer(layer_name)
end

layout:remove_layer("ColemakDH")

-- Set bindings on layer_Cursor using named key positions
local key_bindings = {
	{ pos = "RH_C1R3", binding = "&kp(LEFT)" },
	{ pos = "RH_C2R3", binding = "&kp DOWN" },
	{ pos = "RH_C3R3", binding = "&kp UP" },
	{ pos = "RH_C4R3", binding = "&kp RIGHT" },
	{ pos = "RH_C5R3", binding = "&kp _COPY" },
}

local cursor_layer = layout:layer("Cursor")

for _, entry in ipairs(key_bindings) do
	local index = positions:get(entry.pos)
	if index then
		-- positions:get returns 0-based, :bind expects 1-based
		cursor_layer:bind(index + 1, entry.binding)
		log(string.format("  %s (index %d): %s", entry.pos, index, entry.binding))
	else
		log(string.format("  Warning: unknown position %s", entry.pos))
	end
end

cursor_layer:apply()
