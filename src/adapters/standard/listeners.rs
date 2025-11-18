use std::collections::BTreeMap;

use serde_json::{Number, Value};

use crate::{
    ast::{DtComment, DtItem, DtNode, DtProperty},
    dts::DtsDocument,
};

use super::types::{InputListenerNodeSpec, InputListenerSpec, InputProcessorSpec};

pub(crate) fn extract_input_listeners(document: &DtsDocument) -> Vec<InputListenerSpec> {
    let mut listeners = Vec::new();
    collect_input_listener_nodes(&document.items, &mut listeners);
    listeners
}

fn collect_input_listener_nodes(items: &[DtItem], listeners: &mut Vec<InputListenerSpec>) {
    for item in items {
        match item {
            DtItem::Node(node) => {
                if let Some(listener) = build_input_listener_spec(node) {
                    listeners.push(listener);
                }
                collect_input_listener_nodes(&node.children, listeners);
            }
            DtItem::Conditional(cond) => {
                for branch in &cond.branches {
                    collect_input_listener_nodes(&branch.items, listeners);
                }
            }
            _ => {}
        }
    }
}

fn build_input_listener_spec(node: &DtNode) -> Option<InputListenerSpec> {
    if !is_input_listener_node(node) {
        return None;
    }
    let code = listener_identifier(node)?;
    let (property_order, properties) =
        capture_listener_property_data(&node.properties, &["input-processors"]);
    let mut spec = InputListenerSpec {
        code,
        input_processors: parse_input_processors_from_node(node),
        nodes: Vec::new(),
        properties,
        property_order,
    };
    for child in &node.children {
        if let DtItem::Node(listener_node) = child {
            let (child_order, child_properties) = capture_listener_property_data(
                &listener_node.properties,
                &["layers", "input-processors"],
            );
            let node_spec = InputListenerNodeSpec {
                code: listener_node.name.clone(),
                description: extract_description_from_comments(&listener_node.leading_comments),
                layers: parse_layers_property(listener_node),
                input_processors: parse_input_processors_from_node(listener_node),
                properties: child_properties,
                property_order: child_order,
            };
            spec.nodes.push(node_spec);
        }
    }
    Some(spec)
}

fn is_input_listener_node(node: &DtNode) -> bool {
    let candidates = [node.raw_name.as_str(), node.name.as_str()];
    candidates.iter().any(|candidate| {
        let lowered = candidate.trim().to_ascii_lowercase();
        lowered.contains("input_listener")
    })
}

fn listener_identifier(node: &DtNode) -> Option<String> {
    let candidates = [node.raw_name.as_str(), node.name.as_str()];
    for candidate in candidates {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(start) = trimmed.rfind('&') {
            let remainder = trimmed[start..].split_whitespace().next().unwrap_or("");
            if !remainder.is_empty() {
                return Some(remainder.to_string());
            }
        } else if trimmed.starts_with('&') {
            return Some(trimmed.to_string());
        }
    }
    let fallback = node.name.trim();
    if fallback.is_empty() {
        None
    } else {
        Some(fallback.to_string())
    }
}

fn parse_input_processors_from_node(node: &DtNode) -> Vec<InputProcessorSpec> {
    node.properties
        .iter()
        .find(|prop| prop.name == "input-processors")
        .map(|prop| parse_input_processors_raw(&prop.value.raw))
        .unwrap_or_default()
}

fn parse_input_processors_raw(raw: &str) -> Vec<InputProcessorSpec> {
    split_angle_groups(raw)
        .into_iter()
        .filter_map(|body| parse_input_processor_entry(&body))
        .collect()
}

fn split_angle_groups(raw: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    let bytes = raw.as_bytes();
    for (idx, ch) in bytes.iter().enumerate() {
        match *ch as char {
            '<' => {
                if depth == 0 {
                    start = Some(idx + 1);
                }
                depth += 1;
            }
            '>' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    if let Some(begin) = start.take() {
                        if begin <= idx {
                            groups.push(raw[begin..idx].to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if groups.is_empty() {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            groups.push(trimmed.to_string());
        }
    }
    groups
}

fn parse_input_processor_entry(body: &str) -> Option<InputProcessorSpec> {
    let mut tokens: Vec<&str> = body
        .split_whitespace()
        .map(|token| token.trim_matches(','))
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let code = tokens.remove(0).to_string();
    let params = tokens
        .into_iter()
        .map(parse_input_processor_param_value)
        .collect();
    Some(InputProcessorSpec { code, params })
}

fn parse_input_processor_param_value(token: &str) -> Value {
    let cleaned = token.trim();
    if cleaned.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if cleaned.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if cleaned.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if let Some(stripped) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        if let Ok(value) = u64::from_str_radix(stripped, 16) {
            return Value::Number(Number::from(value));
        }
    }
    if let Some(stripped) = cleaned
        .strip_prefix("0b")
        .or_else(|| cleaned.strip_prefix("0B"))
    {
        if let Ok(value) = u64::from_str_radix(stripped, 2) {
            return Value::Number(Number::from(value));
        }
    }
    if let Ok(value) = cleaned.parse::<i64>() {
        return Value::Number(Number::from(value));
    }
    if cleaned.contains('.') || cleaned.contains('e') || cleaned.contains('E') {
        if let Ok(value) = cleaned.parse::<f64>() {
            if let Some(number) = Number::from_f64(value) {
                return Value::Number(number);
            }
        }
    }
    Value::String(cleaned.to_string())
}

fn parse_layers_property(node: &DtNode) -> Vec<u32> {
    node.properties
        .iter()
        .find(|prop| prop.name == "layers")
        .map(|prop| parse_numeric_list(&prop.value.raw))
        .unwrap_or_default()
}

fn capture_listener_property_data(
    properties: &[DtProperty],
    known: &[&str],
) -> (Vec<String>, BTreeMap<String, String>) {
    let mut order = Vec::new();
    let mut extras = BTreeMap::new();
    for prop in properties {
        order.push(prop.name.clone());
        if known.contains(&prop.name.as_str()) {
            continue;
        }
        extras.insert(prop.name.clone(), normalize_property_value(&prop.value.raw));
    }
    (order, extras)
}

fn parse_numeric_list(raw: &str) -> Vec<u32> {
    raw.trim_matches(|ch| matches!(ch, '<' | '>' | ';'))
        .split_whitespace()
        .filter_map(|token| {
            if let Some(stripped) = token
                .strip_prefix("0x")
                .or_else(|| token.strip_prefix("0X"))
            {
                u32::from_str_radix(stripped, 16).ok()
            } else if let Some(stripped) = token
                .strip_prefix("0b")
                .or_else(|| token.strip_prefix("0B"))
            {
                u32::from_str_radix(stripped, 2).ok()
            } else {
                token.parse().ok()
            }
        })
        .collect()
}

fn extract_description_from_comments(comments: &[DtComment]) -> Option<String> {
    let lines = collect_comment_lines(comments)?;
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn clean_comment_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let content = if trimmed.starts_with("//") {
        trimmed
            .trim_start_matches('/')
            .trim_start_matches('/')
            .trim()
    } else if trimmed.starts_with("/*") {
        trimmed
            .trim_start_matches("/*")
            .trim_end_matches("*/")
            .trim()
    } else {
        trimmed
    };
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

fn collect_comment_lines(comments: &[DtComment]) -> Option<Vec<String>> {
    let mut lines = Vec::new();
    let mut started = false;
    for comment in comments.iter().rev() {
        match clean_comment_text(&comment.text) {
            Some(text) => {
                started = true;
                lines.push(text);
            }
            None => {
                if started {
                    lines.push(String::new());
                }
            }
        }
    }
    if !started {
        return None;
    }
    lines.reverse();
    let start = lines.iter().position(|line| !line.is_empty()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|line| !line.is_empty())
        .unwrap_or(start);
    Some(lines[start..=end].to_vec())
}

fn normalize_property_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.trim_end_matches(';').trim().to_string()
    }
}
