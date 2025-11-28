use crate::{bindings::BindingParser, tokenizer::TokenSpan};

use super::ProviderError;

/// Shared helpers for parsing/formatting bindings and numeric lists.
pub struct BindingFormat<'a> {
    parser: &'a BindingParser,
}

impl<'a> BindingFormat<'a> {
    pub fn new(parser: &'a BindingParser) -> Self {
        Self { parser }
    }

    /// Normalize a single binding string using the shared parser.
    pub fn normalize_binding(&self, binding: &str) -> Result<String, ProviderError> {
        if binding.trim().is_empty() {
            return Err(ProviderError::InvalidBinding(
                "binding string cannot be empty".to_string(),
            ));
        }
        Ok(self
            .parser
            .parse_with_behavior_rules(binding)
            .to_binding_string())
    }

    /// Normalize multiple bindings, ensuring at least one is present.
    pub fn normalize_bindings(&self, bindings: &[&str]) -> Result<Vec<String>, ProviderError> {
        if bindings.is_empty() {
            return Err(ProviderError::InvalidBinding(
                "at least one binding is required".to_string(),
            ));
        }
        bindings
            .iter()
            .map(|binding| self.normalize_binding(binding))
            .collect()
    }
}

pub fn parse_binding_groups(raw: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in raw.chars() {
        match ch {
            '<' => {
                if depth == 0 {
                    current.clear();
                } else {
                    current.push(ch);
                }
                depth += 1;
            }
            '>' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        let trimmed = current.trim();
                        if !trimmed.is_empty() {
                            if trimmed.matches('&').count() > 1 {
                                groups.extend(split_binding_sequence(trimmed));
                            } else {
                                groups.push(trimmed.to_string());
                            }
                        }
                    } else {
                        current.push(ch);
                    }
                }
            }
            _ => {
                if depth > 0 {
                    current.push(ch);
                }
            }
        }
    }
    if groups.is_empty() {
        let trimmed = raw
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim_end_matches(';')
            .trim();
        if !trimmed.is_empty() {
            if trimmed.matches('&').count() > 1 {
                groups.extend(split_binding_sequence(trimmed));
            } else {
                groups.push(trimmed.to_string());
            }
        }
    }
    groups
}

pub fn parse_binding_list(raw: &str) -> Vec<String> {
    parse_binding_groups(raw)
}

pub fn split_binding_sequence(sequence: &str) -> Vec<String> {
    let mut bindings = Vec::new();
    let mut current = String::new();
    for token in sequence.split_whitespace() {
        if token.starts_with('&') {
            if !current.is_empty() {
                bindings.push(current.trim().to_string());
                current.clear();
            }
            current.push_str(token);
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(token);
        }
    }
    if !current.is_empty() {
        bindings.push(current.trim().to_string());
    }
    bindings
}

pub fn format_bindings_raw(bindings: &[String]) -> String {
    format_list(bindings)
}

pub fn format_list(values: &[String]) -> String {
    if values.is_empty() {
        "< >".to_string()
    } else {
        format!("< {} >", values.join(" "))
    }
}

pub fn format_u32_list(values: &[u32]) -> String {
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

pub fn parse_numeric_list(raw: &str) -> Vec<u32> {
    raw.replace('<', " ")
        .replace('>', " ")
        .replace(';', " ")
        .replace(',', " ")
        .split_whitespace()
        .filter_map(parse_u32_token)
        .collect()
}

pub fn parse_numeric_value(raw: &str) -> Option<u32> {
    raw.trim_matches(['<', '>', ';', ' ']).parse().ok()
}

pub fn parse_u32_token(token: &str) -> Option<u32> {
    if token.is_empty() {
        return None;
    }
    if let Some(stripped) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        return u32::from_str_radix(stripped, 16).ok();
    }
    if let Some(stripped) = token
        .strip_prefix("0b")
        .or_else(|| token.strip_prefix("0B"))
    {
        return u32::from_str_radix(stripped, 2).ok();
    }
    token.parse::<u32>().ok()
}

pub fn empty_span() -> TokenSpan {
    TokenSpan::new(0, 0, 1, 1, 1, 1)
}
