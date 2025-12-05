//! Shared formatting helpers for rendering layout bindings in a grid.
//!
//! These utilities are used by template rendering and CLI helpers to display
//! layers using the keyboard profile's formatting hints.

/// Format a layer's bindings into aligned lines using the provided row layout.
///
/// * `bindings` - list of bindings for the layer (by key position index)
/// * `rows` - ordered key positions for each row; negative values render as gaps
/// * `key_gap` - spacing inserted between columns
/// * `base_indent` - optional indent prefix applied before each row (normalized away)
pub fn render_layer_lines(
    bindings: &[String],
    rows: &[Vec<i32>],
    key_gap: &str,
    base_indent: &str,
) -> Vec<String> {
    if rows.is_empty() {
        return Vec::new();
    }

    let mut width_per_col: Vec<usize> = Vec::new();
    for row in rows {
        for (idx, pos) in row.iter().enumerate() {
            let token = binding_for_pos(bindings, *pos);
            let len = token.as_ref().map(|t| t.len()).unwrap_or_default();
            if width_per_col.len() <= idx {
                width_per_col.push(len);
            } else if len > width_per_col[idx] {
                width_per_col[idx] = len;
            }
        }
    }

    let mut lines: Vec<String> = Vec::new();
    for row in rows {
        let mut line = String::new();
        line.push_str(base_indent);
        for (col, pos) in row.iter().enumerate() {
            if col > 0 {
                line.push_str(key_gap);
            }
            let width = *width_per_col.get(col).unwrap_or(&0);
            if let Some(tok) = binding_for_pos(bindings, *pos) {
                line.push_str(&format!("{tok:>width$}"));
            } else {
                line.push_str(&" ".repeat(width.max(1)));
            }
        }
        lines.push(line);
    }

    let min_indent = lines
        .iter()
        .map(|line| line.chars().take_while(|c| c.is_whitespace()).count())
        .min()
        .unwrap_or(0);
    lines
        .into_iter()
        .map(|line| line.chars().skip(min_indent).collect())
        .collect()
}

/// Render a layer block wrapped in angle brackets, matching DTS formatting.
pub fn render_layer_block(
    bindings: &[String],
    rows: &[Vec<i32>],
    key_gap: &str,
    base_indent: &str,
) -> String {
    if rows.is_empty() {
        return "< >".to_string();
    }
    let lines = render_layer_lines(bindings, rows, key_gap, base_indent);
    if lines.is_empty() {
        return "< >".to_string();
    }
    let joined = lines.join("\n");
    format!("<\n{joined}\n{base_indent}>")
}

/// Normalize a binding string to a compact, modifier-wrapped representation.
///
/// Example: `&kp LSFT LG A` becomes `&kp LG(LSFT(A))`
pub fn format_binding(raw: &str) -> String {
    let tokens: Vec<&str> = raw.split_whitespace().filter(|t| !t.is_empty()).collect();
    if tokens.len() < 2 {
        return raw.to_string();
    }
    let head = tokens[0];
    let modifiers = &tokens[1..tokens.len() - 1];
    let base = tokens.last().copied().unwrap_or_default();
    if !head.starts_with('&') || base.contains('(') || base.contains(')') {
        return raw.to_string();
    }
    if !modifiers.iter().all(|m| is_known_modifier(m)) {
        return raw.to_string();
    }
    let mut wrapped = base.to_string();
    for modifier in modifiers.iter().rev() {
        wrapped = format!("{modifier}({wrapped})");
    }
    format!("{head} {wrapped}")
}

fn binding_for_pos(bindings: &[String], pos: i32) -> Option<String> {
    if pos < 0 {
        return None;
    }
    bindings.get(pos as usize).map(|raw| format_binding(raw))
}

fn is_known_modifier(token: &str) -> bool {
    matches!(
        token,
        "LC" | "RC"
            | "LS"
            | "RS"
            | "LG"
            | "RG"
            | "LA"
            | "RA"
            | "LALT"
            | "RALT"
            | "LCTL"
            | "RCTL"
            | "LSFT"
            | "RSFT"
            | "LGUI"
            | "RGUI"
    )
}
