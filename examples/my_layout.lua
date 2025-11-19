local layers = list_layers()

for _, layer in ipairs(layers) do
    local define_name = layer.name
    if string.sub(define_name, 1, 6) == "layer_" then
        define_name = string.sub(define_name, 7)
    end
    if string.upper(define_name) == "QWERTY" then
        break
    end

    log(string.format("Removing layer: %s", layer.name))
    remove_layer(layer.name)
end

local bindings = { "&kp LEFT", "&kp DOWN", "&kp UP", "&kp RIGHT", "&kp COPY" }
for index, binding in ipairs(bindings) do
    local position = 31 + index
    set_binding("layer_Cursor", position, binding)
    log(string.format("  Position %d: %s", position, binding))
end
