use std::collections::{BTreeMap, BTreeSet, HashSet};

use minijinja::{AutoEscape, Environment};
use serde_json::{Map, Value};

use crate::formatting::{format_binding, render_layer_block};

use super::{
    layout::AdapterLayout,
    template::TemplateError,
    types::{
        BehaviorSpec, ComboSpec, InputListenerNodeSpec, InputListenerSpec, InputProcessorSpec,
        LayerSpec, MacroSpec,
    },
};

#[derive(Debug)]
struct FormattingHints {
    rows: Vec<Vec<i32>>,
    key_gap: String,
    base_indent: String,
    layer_prefix: String,
    combo_prefix: String,
}

impl FormattingHints {
    fn from_extras(extras: &BTreeMap<String, Value>) -> Option<Self> {
        let rows_value = extras.get("formatting_rows")?;
        let rows = rows_value.as_array()?.iter().filter_map(|row| {
            row.as_array().map(|entries| {
                entries
                    .iter()
                    .filter_map(|val| val.as_i64().map(|v| v as i32))
                    .collect::<Vec<_>>()
            })
        });
        let collected: Vec<Vec<i32>> = rows.collect();
        if collected.is_empty() {
            return None;
        }
        let key_gap = extras
            .get("formatting_key_gap")
            .and_then(|v| v.as_str())
            .unwrap_or("  ")
            .to_string();
        let base_indent = extras
            .get("formatting_base_indent")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let layer_prefix = extras
            .get("formatting_layer_prefix")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let combo_prefix = extras
            .get("formatting_combo_prefix")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Some(Self {
            rows: collected,
            key_gap,
            base_indent,
            layer_prefix,
            combo_prefix,
        })
    }
}

pub fn render_layout_with_template(
    layout: &AdapterLayout,
    template: &str,
) -> Result<String, TemplateError> {
    let formatting = FormattingHints::from_extras(&layout.metadata.extras);
    let mut replacements = build_template_replacements(layout, formatting.as_ref());
    let has_explicit_includes = layout.metadata.extras.contains_key("includes");
    normalize_template_includes(template, &mut replacements, has_explicit_includes);
    let context = build_template_context(layout, replacements, formatting.as_ref())?;

    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_keep_trailing_newline(true);

    Ok(env.render_str(template, &context)?)
}

fn build_template_replacements(
    layout: &AdapterLayout,
    formatting: Option<&FormattingHints>,
) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let extras = &layout.metadata.extras;

    let keyboard_name = metadata_text(extras, "keyboard_name")
        .or_else(|| layout.metadata.title.clone())
        .unwrap_or_else(|| "ZMK Layout".to_string());
    insert_placeholder(&mut map, "keyboard_name", keyboard_name, extras);

    let includes = metadata_text(extras, "includes")
        .or_else(|| metadata_text(extras, "resolved_includes"))
        .unwrap_or_default();
    insert_placeholder(&mut map, "includes", includes.clone(), extras);
    insert_placeholder(&mut map, "resolved_includes", includes, extras);

    let defines = metadata_text(extras, "defines").unwrap_or_default();
    insert_placeholder(&mut map, "defines", defines, extras);

    let layer_defines = render_layer_defines(&layout.layers);
    insert_placeholder(
        &mut map,
        "layer_names_defines",
        layer_defines.clone(),
        extras,
    );
    insert_placeholder(&mut map, "layer_defines", layer_defines, extras);

    let rendered_layers = render_layers_only(layout, formatting);
    insert_placeholder(&mut map, "rendered_layers", rendered_layers, extras);

    let keymap_node = render_keymap_node(layout, formatting);
    insert_placeholder(&mut map, "keymap_node", keymap_node, extras);

    let macros_block = render_macros(&layout.macros);
    insert_placeholder(&mut map, "macros", macros_block.clone(), extras);
    insert_placeholder(&mut map, "user_macros_dtsi", macros_block, extras);

    let behaviors_block = render_behaviors(layout.behaviors.iter());
    insert_placeholder(&mut map, "behaviors", behaviors_block.clone(), extras);
    insert_placeholder(&mut map, "user_behaviors_dtsi", behaviors_block, extras);

    if let Some((combos_root, combos_body)) = render_combos(&layout.combos, formatting) {
        insert_placeholder(&mut map, "combos", combos_root, extras);
        insert_placeholder(&mut map, "combos_dtsi", combos_body, extras);
    } else {
        insert_placeholder(&mut map, "combos", String::new(), extras);
        insert_placeholder(&mut map, "combos_dtsi", String::new(), extras);
    }

    let rendered_input_listeners = render_input_listeners(&layout.input_listeners);
    for key in [
        "custom_devicetree",
        "input_listeners",
        "input_listeners_dtsi",
        "custom_defined_behaviors",
        "input_processors",
        "system_behaviors_dts",
        "key_position_header",
        "custom_defined_macros",
        "keycode_locale_definitions",
        "key_position_defines",
    ] {
        let value = match metadata_text(extras, key) {
            Some(text) => text,
            None if key == "input_listeners" || key == "input_listeners_dtsi" => {
                rendered_input_listeners.clone()
            }
            None => String::new(),
        };
        insert_placeholder(&mut map, key, value, extras);
    }

    map
}

fn normalize_template_includes(
    template: &str,
    replacements: &mut BTreeMap<String, String>,
    has_explicit_includes: bool,
) {
    if has_explicit_includes {
        return;
    }
    let template_includes = collect_template_include_lines(template);
    if template_includes.is_empty() {
        return;
    }
    let Some(current_value) = replacements
        .get("includes")
        .cloned()
        .or_else(|| replacements.get("resolved_includes").cloned())
    else {
        return;
    };
    if current_value.trim().is_empty() {
        return;
    }
    let filtered = filter_includes_not_in_template(&current_value, &template_includes);
    for key in ["includes", "resolved_includes"] {
        if replacements.contains_key(key) {
            replacements.insert(key.to_string(), filtered.clone());
        }
    }
}

fn build_template_context(
    layout: &AdapterLayout,
    placeholders: BTreeMap<String, String>,
    formatting: Option<&FormattingHints>,
) -> Result<Value, TemplateError> {
    let mut context = Map::new();
    let mut content = Map::new();
    for (key, value) in placeholders {
        let json_value = Value::String(value);
        content.insert(key.clone(), json_value.clone());
        context.insert(key, json_value);
    }

    context.insert("content".into(), Value::Object(content));
    let mut layout_value = serde_json::to_value(layout)?;
    if let (Some(fmt), Some(obj)) = (formatting, layout_value.as_object_mut()) {
        obj.insert(
            "formatting".into(),
            serde_json::json!({
                "layerPrefix": fmt.layer_prefix,
                "comboPrefix": fmt.combo_prefix
            }),
        );
    }
    context.insert("layout".into(), layout_value);

    Ok(Value::Object(context))
}

fn collect_template_include_lines(template: &str) -> BTreeSet<String> {
    template
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("#include") && !trimmed.contains("{{") {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn filter_includes_not_in_template(value: &str, template_lines: &BTreeSet<String>) -> String {
    let mut filtered = Vec::new();
    for line in value.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !template_lines.contains(trimmed) {
            filtered.push(line.to_string());
        }
    }

    let mut result = filtered.join("\n");
    if value.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn insert_placeholder(
    map: &mut BTreeMap<String, String>,
    key: &str,
    value: String,
    extras: &BTreeMap<String, Value>,
) {
    for name in placeholder_names(extras, key) {
        map.insert(name, value.clone());
    }
}

fn placeholder_names(extras: &BTreeMap<String, Value>, key: &str) -> Vec<String> {
    const DEFAULT_ALIASES: &[(&str, &[&str])] =
        &[("key_position_header", &["key_position_defines"])];

    let mut names = vec![key.to_string()];

    if let Some((_, aliases)) = DEFAULT_ALIASES
        .iter()
        .find(|(canonical, _)| canonical == &key)
    {
        names.extend(aliases.iter().map(|alias| alias.to_string()));
    }

    let alias_key = format!("placeholder_alias_{key}");
    if let Some(value) = extras.get(&alias_key) {
        push_alias_value(&mut names, value);
    }

    if let Some(map) = extras
        .get("template_placeholders")
        .and_then(|v| v.as_object())
    {
        if let Some(value) = map.get(key) {
            push_alias_value(&mut names, value);
        }
    }

    names.sort();
    names.dedup();
    names
}

fn push_alias_value(names: &mut Vec<String>, value: &Value) {
    match value {
        Value::String(alias) => {
            if !alias.trim().is_empty() {
                names.push(alias.clone());
            }
        }
        Value::Array(values) => {
            for entry in values {
                if let Value::String(alias) = entry {
                    if !alias.trim().is_empty() {
                        names.push(alias.clone());
                    }
                }
            }
        }
        _ => {}
    }
}

fn metadata_text(extras: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    extras.get(key).map(|value| match value {
        Value::String(text) => text.clone(),
        Value::Number(num) => num.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Number(num) => Some(num.to_string()),
                Value::Bool(flag) => Some(flag.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(obj) => serde_json::to_string_pretty(obj).unwrap_or_default(),
        Value::Null => String::new(),
    })
}

fn render_layer_defines(layers: &[LayerSpec]) -> String {
    if layers.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for (idx, layer) in layers.iter().enumerate() {
        let define_name = sanitize_define_name(&layer.name);
        output.push_str(&format!("#define LAYER_{} {}\n", define_name, idx));
    }
    if output.ends_with('\n') {
        output.pop();
    }
    output
}

fn render_layers_only(layout: &AdapterLayout, formatting: Option<&FormattingHints>) -> String {
    if layout.layers.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for layer in &layout.layers {
        output.push_str("        ");
        let name = if let Some(fmt) = formatting.as_ref() {
            format!(
                "{}{}",
                fmt.layer_prefix,
                sanitize_node_identifier(&layer.name)
            )
        } else {
            sanitize_node_identifier(&layer.name)
        };
        output.push_str(&name);
        output.push_str(" {\n");
        output.push_str("            bindings = ");
        match formatting.as_ref() {
            Some(fmt) => output.push_str(&render_layer_block(
                &layer.bindings,
                &fmt.rows,
                &fmt.key_gap,
                &fmt.base_indent,
            )),
            None => {
                output.push_str(&format_list(&layer.bindings));
            }
        }
        output.push_str(";");
        output.push_str("\n        };\n");
    }
    output
}

fn render_keymap_node(layout: &AdapterLayout, formatting: Option<&FormattingHints>) -> String {
    let mut output = String::new();
    output.push_str("keymap {\n    compatible = \"zmk,keymap\";\n");
    if !layout.layers.is_empty() {
        let rendered = render_layers_only(layout, formatting);
        output.push('\n');
        output.push_str(rendered.trim_end());
        output.push('\n');
    }
    output.push_str("};\n");
    output
}

fn render_behaviors<'a>(behaviors: impl Iterator<Item = &'a BehaviorSpec>) -> String {
    let mut blocks = Vec::new();
    for behavior in behaviors {
        let mut block = String::new();
        push_comment_lines(&mut block, "        ", &behavior.description);
        block.push_str("        ");
        block.push_str(&behavior.name);
        block.push_str(": ");
        block.push_str(&behavior.name);
        block.push_str(" {\n");
        let mut rendered = HashSet::new();
        for name in behavior.resolved_property_order() {
            if render_behavior_property(behavior, &name, &mut block) {
                rendered.insert(name);
            }
        }
        for name in behavior.default_property_order() {
            if rendered.contains(&name) {
                continue;
            }
            if render_behavior_property(behavior, &name, &mut block) {
                rendered.insert(name);
            }
        }
        block.push_str("        };\n");
        blocks.push(block);
    }
    blocks.join("\n")
}

fn render_behavior_property(behavior: &BehaviorSpec, name: &str, block: &mut String) -> bool {
    match name {
        "label" => {
            if let Some(label) = behavior.label.as_deref() {
                block.push_str("            label = \"");
                block.push_str(label);
                block.push_str("\";\n");
                return true;
            }
        }
        "compatible" => {
            if let Some(compat) = &behavior.compatible {
                block.push_str("            compatible = \"");
                block.push_str(compat);
                block.push_str("\";\n");
                return true;
            }
        }
        "#binding-cells" => {
            if let Some(binding_cells) = behavior.binding_cells {
                block.push_str("            #binding-cells = <");
                block.push_str(&binding_cells.to_string());
                block.push_str(">;\n");
                return true;
            }
        }
        "bindings" => {
            if let Some(rendered) = render_behavior_bindings(&behavior.bindings) {
                block.push_str(&rendered);
                return true;
            }
        }
        _ => {
            if let Some(value) = behavior.properties.get(name) {
                block.push_str("            ");
                block.push_str(name);
                if value.trim().is_empty() {
                    block.push_str(";\n");
                } else {
                    block.push_str(" = ");
                    block.push_str(value);
                    block.push_str(";\n");
                }
                return true;
            }
        }
    }
    false
}

fn render_behavior_bindings(bindings: &[String]) -> Option<String> {
    if bindings.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for binding in bindings {
        let trimmed = binding.trim();
        if trimmed.is_empty() {
            continue;
        }
        parts.push(format!("<{}>", trimmed));
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("            bindings = {};\n", parts.join(", ")))
}

fn push_comment_lines(block: &mut String, indent: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    for line in text.split('\n') {
        let trimmed = line.trim();
        block.push_str(indent);
        block.push_str("//");
        if !trimmed.is_empty() {
            block.push(' ');
            block.push_str(trimmed);
        }
        block.push('\n');
    }
}

fn push_comment_lines_with_depth(block: &mut String, depth: usize, text: &str) {
    let indent = "    ".repeat(depth);
    push_comment_lines(block, &indent, text);
}

fn render_combos(
    combos: &[ComboSpec],
    formatting: Option<&FormattingHints>,
) -> Option<(String, String)> {
    if combos.is_empty() {
        return None;
    }
    let mut inner = String::new();
    inner.push_str("combos {\n");
    inner.push_str("    compatible = \"zmk,combos\";\n");
    for combo in combos {
        let node_name = sanitize_node_identifier(&combo.name);
        let node_name = if let Some(fmt) = formatting.as_ref() {
            format!("{}{}", fmt.combo_prefix, node_name)
        } else {
            node_name
        };
        push_comment_lines(&mut inner, "    ", &combo.description);
        inner.push_str("    ");
        inner.push_str(&node_name);
        inner.push_str(" {\n");
        let mut rendered = HashSet::new();
        for name in combo.resolved_property_order() {
            if render_combo_property(combo, &name, &mut inner) {
                rendered.insert(name);
            }
        }
        for name in combo.default_property_order() {
            if rendered.contains(&name) {
                continue;
            }
            if render_combo_property(combo, &name, &mut inner) {
                rendered.insert(name);
            }
        }
        inner.push_str("    };\n\n");
    }
    if inner.ends_with("\n\n") {
        inner.truncate(inner.len() - 1);
    }
    inner.push_str("};\n");

    let combos_root = format!("/ {{\n{}\n}};\n", indent_block(inner.trim_end(), 4));
    Some((combos_root, inner))
}

fn render_combo_property(combo: &ComboSpec, name: &str, block: &mut String) -> bool {
    match name {
        "timeout-ms" => {
            if let Some(timeout) = combo.timeout_ms {
                block.push_str("        timeout-ms = <");
                block.push_str(&timeout.to_string());
                block.push_str(">;\n");
                return true;
            }
        }
        "key-positions" => {
            if !combo.key_positions.is_empty() {
                block.push_str("        key-positions = <");
                block.push_str(
                    &combo
                        .key_positions
                        .iter()
                        .map(|pos| pos.to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                block.push_str(">;\n");
                return true;
            }
        }
        "bindings" => {
            if let Some(binding_line) = render_combo_bindings(combo) {
                block.push_str(&binding_line);
                return true;
            }
        }
        "layers" => {
            if !combo.layers.is_empty() {
                block.push_str("        layers = ");
                block.push_str(&format_compact_u32_list(&combo.layers));
                block.push_str(";\n");
                return true;
            }
        }
        _ => {
            if let Some(value) = combo.property_value(name) {
                block.push_str("        ");
                block.push_str(name);
                if value.trim().is_empty() {
                    block.push_str(";\n");
                } else {
                    block.push_str(" = ");
                    block.push_str(value);
                    block.push_str(";\n");
                }
                return true;
            }
        }
    }
    false
}

fn render_combo_bindings(combo: &ComboSpec) -> Option<String> {
    let binding = combo.binding.as_deref()?.trim();
    if binding.is_empty() {
        return None;
    }
    Some(format!(
        "        bindings = <{}>;\n",
        format_binding(binding)
    ))
}

fn render_macros(macros: &[MacroSpec]) -> String {
    if macros.is_empty() {
        return String::new();
    }
    let mut blocks = Vec::new();
    for macro_behavior in macros {
        let mut block = String::new();
        push_comment_lines(&mut block, "        ", &macro_behavior.description);
        block.push_str("        ");
        block.push_str(&macro_behavior.name);
        block.push_str(": ");
        block.push_str(&macro_behavior.name);
        block.push_str(" {\n");
        let mut rendered = HashSet::new();
        for name in macro_behavior.resolved_property_order() {
            if render_macro_property(macro_behavior, &name, &mut block) {
                rendered.insert(name);
            }
        }
        for name in macro_behavior.default_property_order() {
            if rendered.contains(&name) {
                continue;
            }
            if render_macro_property(macro_behavior, &name, &mut block) {
                rendered.insert(name);
            }
        }
        block.push_str("        };\n");
        blocks.push(block);
    }
    blocks.join("\n")
}

fn render_macro_property(macro_behavior: &MacroSpec, name: &str, block: &mut String) -> bool {
    match name {
        "label" => {
            if let Some(label) = macro_behavior.label.as_deref() {
                block.push_str("            label = \"");
                block.push_str(label);
                block.push_str("\";\n");
                return true;
            }
        }
        "compatible" => {
            let compatible = macro_behavior
                .compatible
                .as_deref()
                .unwrap_or("zmk,behavior-macro");
            block.push_str("            compatible = \"");
            block.push_str(compatible);
            block.push_str("\";\n");
            return true;
        }
        "#binding-cells" => {
            let binding_cells = macro_behavior
                .binding_cells
                .or_else(|| Some(0))
                .unwrap_or(0);
            block.push_str("            #binding-cells = <");
            block.push_str(&binding_cells.to_string());
            block.push_str(">;\n");
            return true;
        }
        "tap-ms" => {
            if let Some(tap) = macro_behavior.tap_ms {
                block.push_str("            tap-ms = <");
                block.push_str(&tap.to_string());
                block.push_str(">;\n");
                return true;
            }
        }
        "wait-ms" => {
            if let Some(wait) = macro_behavior.wait_ms {
                block.push_str("            wait-ms = <");
                block.push_str(&wait.to_string());
                block.push_str(">;\n");
                return true;
            }
        }
        "bindings" => {
            if render_macro_bindings(macro_behavior, block) {
                return true;
            }
        }
        _ => {
            if let Some(value) = macro_behavior.property_value(name) {
                block.push_str("            ");
                block.push_str(name);
                if value.trim().is_empty() {
                    block.push_str(";\n");
                } else {
                    block.push_str(" = ");
                    block.push_str(value);
                    block.push_str(";\n");
                }
                return true;
            }
        }
    }
    false
}

fn render_macro_bindings(macro_behavior: &MacroSpec, block: &mut String) -> bool {
    if macro_behavior.bindings.is_empty() {
        return false;
    }
    let formatted: Vec<String> = macro_behavior
        .bindings
        .iter()
        .map(|b| format_binding(b))
        .collect();
    block.push_str("            bindings = <");
    block.push_str(formatted[0].trim());
    block.push_str(">");
    if formatted.len() == 1 {
        block.push_str(";\n");
    } else {
        block.push('\n');
        let last_index = formatted.len() - 1;
        for (idx, binding) in formatted.iter().enumerate().skip(1) {
            block.push_str("                , <");
            block.push_str(binding.trim());
            block.push('>');
            if idx == last_index {
                block.push_str(";\n");
            } else {
                block.push('\n');
            }
        }
    }
    true
}

fn render_input_listeners(listeners: &[InputListenerSpec]) -> String {
    if listeners.is_empty() {
        return String::new();
    }
    let mut blocks = Vec::new();
    for listener in listeners {
        let mut block = String::new();
        block.push_str(listener.code.trim());
        block.push_str(" {\n");
        let mut rendered = HashSet::new();
        for name in listener.resolved_property_order() {
            if render_input_listener_property(listener, &name, &mut block) {
                rendered.insert(name);
            }
        }
        for name in listener.default_property_order() {
            if rendered.contains(&name) {
                continue;
            }
            if render_input_listener_property(listener, &name, &mut block) {
                rendered.insert(name);
            }
        }
        for node in &listener.nodes {
            if let Some(description) = node.description.as_ref() {
                push_comment_lines_with_depth(&mut block, 1, description);
            }
            append_listener_indent(&mut block, 1);
            block.push_str(&node.code);
            block.push_str(" {\n");
            let mut rendered_node = HashSet::new();
            for name in node.resolved_property_order() {
                if render_input_listener_node_property(node, &name, &mut block) {
                    rendered_node.insert(name);
                }
            }
            for name in node.default_property_order() {
                if rendered_node.contains(&name) {
                    continue;
                }
                if render_input_listener_node_property(node, &name, &mut block) {
                    rendered_node.insert(name);
                }
            }
            append_listener_indent(&mut block, 1);
            block.push_str("};\n");
        }
        block.push_str("};\n");
        blocks.push(block);
    }
    blocks.join("\n")
}

fn render_input_listener_property(
    listener: &InputListenerSpec,
    name: &str,
    block: &mut String,
) -> bool {
    match name {
        "input-processors" => {
            if listener.input_processors.is_empty() {
                return false;
            }
            append_listener_indent(block, 1);
            block.push_str("input-processors = ");
            block.push_str(&render_input_processor_list(&listener.input_processors));
            block.push_str(";\n");
            true
        }
        _ => {
            if let Some(value) = listener.properties.get(name) {
                append_listener_indent(block, 1);
                block.push_str(name);
                if value.trim().is_empty() {
                    block.push_str(";\n");
                } else {
                    block.push_str(" = ");
                    block.push_str(value);
                    block.push_str(";\n");
                }
                true
            } else {
                false
            }
        }
    }
}

fn render_input_listener_node_property(
    node: &InputListenerNodeSpec,
    name: &str,
    block: &mut String,
) -> bool {
    match name {
        "layers" => {
            if node.layers.is_empty() {
                return false;
            }
            append_listener_indent(block, 2);
            block.push_str("layers = <");
            block.push_str(
                &node
                    .layers
                    .iter()
                    .map(|layer| layer.to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            block.push_str(">;\n");
            true
        }
        "input-processors" => {
            if node.input_processors.is_empty() {
                return false;
            }
            append_listener_indent(block, 2);
            block.push_str("input-processors = ");
            block.push_str(&render_input_processor_list(&node.input_processors));
            block.push_str(";\n");
            true
        }
        _ => {
            if let Some(value) = node.properties.get(name) {
                append_listener_indent(block, 2);
                block.push_str(name);
                if value.trim().is_empty() {
                    block.push_str(";\n");
                } else {
                    block.push_str(" = ");
                    block.push_str(value);
                    block.push_str(";\n");
                }
                true
            } else {
                false
            }
        }
    }
}

fn append_listener_indent(buffer: &mut String, depth: usize) {
    for _ in 0..depth {
        buffer.push_str("    ");
    }
}

fn render_input_processor_list(processors: &[InputProcessorSpec]) -> String {
    processors
        .iter()
        .map(|processor| {
            let mut tokens = Vec::new();
            tokens.push(processor.code.trim().to_string());
            for param in &processor.params {
                let rendered = render_input_processor_param(param);
                if !rendered.is_empty() {
                    tokens.push(rendered);
                }
            }
            format!("<{}>", tokens.join(" "))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_input_processor_param(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(num) => num.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn format_list(items: &[String]) -> String {
    if items.is_empty() {
        "< >".to_string()
    } else {
        format!("< {} >", items.join(" "))
    }
}

fn indent_block(text: &str, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_compact_u32_list(values: &[u32]) -> String {
    if values.is_empty() {
        "< >".to_string()
    } else {
        format!(
            "< {} >",
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn sanitize_define_name(name: &str) -> String {
    let trimmed = if name.len() >= 6 && name[..6].eq_ignore_ascii_case("layer_") {
        &name[6..]
    } else {
        name
    };
    let mut result = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch);
            continue;
        }
        result.push('_');
    }
    if result.is_empty() {
        "LAYER".to_string()
    } else {
        result
    }
}

fn sanitize_node_identifier(name: &str) -> String {
    let mut result = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            result.push(ch);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        "node".to_string()
    } else {
        result
    }
}
