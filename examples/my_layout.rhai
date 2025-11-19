let layers = list_layers();

for layer in layers {
    // Remove the "layer_" prefix to get the define name
    let define_name = layer.name;
    if define_name.starts_with("layer_") {
        define_name = define_name.sub_string(6);  // Remove "layer_" prefix
    }
    if define_name.to_upper() == "QWERTY" {
      break;
    }

    log(`Removing layer: ${layer.name}`);
    remove_layer(layer.name);
}

let bindings = [ "&kp LEFT", "&kp DOWN", "&kp UP", "&kp RIGHT", "&kp COPY" ];
for i in 0..bindings.len() {
    set_binding("layer_Cursor", 32 + i, bindings[i]);
    log(`  Position ${32 + i}: ${bindings[i]}`);
}
