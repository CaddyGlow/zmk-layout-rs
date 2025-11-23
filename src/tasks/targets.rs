/// Normalize a task locator string by removing whitespace.
pub fn normalize_locator(value: &str) -> String {
    value.split_whitespace().collect::<String>()
}

/// Return the first existing target that overlaps the candidate (exact or hierarchical match).
pub fn find_overlapping_target(existing: &[String], candidate: &str) -> Option<String> {
    for entry in existing {
        if targets_overlap(entry, candidate) {
            return Some(entry.clone());
        }
    }
    None
}

/// Parse an override path (`layers.<name>.bindings[<index>]`) into its parts.
pub fn parse_override_path(path: &str) -> Result<(String, usize), String> {
    const LAYERS_PREFIX: &str = "layers.";
    const BINDINGS_SEGMENT: &str = ".bindings";
    if !path.starts_with(LAYERS_PREFIX) {
        return Err(format!(
            "override path `{}` must start with `layers.`",
            path
        ));
    }
    let rest = &path[LAYERS_PREFIX.len()..];
    let bindings_idx = rest
        .find(BINDINGS_SEGMENT)
        .ok_or_else(|| format!("override path `{}` missing `.bindings`", path))?;
    let layer = rest[..bindings_idx].trim();
    if layer.is_empty() {
        return Err(format!("override path `{}` missing layer name", path));
    }
    let index_part = rest[bindings_idx + BINDINGS_SEGMENT.len()..].trim();
    if !index_part.starts_with('[') || !index_part.ends_with(']') {
        return Err(format!(
            "override path `{}` must include `[index]` after `.bindings`",
            path
        ));
    }
    let index_str = &index_part[1..index_part.len() - 1];
    let index = index_str
        .parse::<usize>()
        .map_err(|_| format!("override path `{}` has invalid index", path))?;
    Ok((layer.to_string(), index))
}

fn targets_overlap(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.starts_with(b) {
        return has_boundary(a, b.len());
    }
    if b.starts_with(a) {
        return has_boundary(b, a.len());
    }
    false
}

fn has_boundary(text: &str, prefix_len: usize) -> bool {
    if text.len() == prefix_len {
        return true;
    }
    text[prefix_len..]
        .chars()
        .next()
        .map(|ch| matches!(ch, '.' | '['))
        .unwrap_or(false)
}
