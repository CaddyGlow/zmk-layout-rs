//! Macro and preprocessor helpers.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::ast::{DtConditional, DtItem, DtMacro};
use crate::tokenizer::TokenSpan;

/// Metadata captured for each macro definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDefinition {
    pub name: String,
    pub args: Vec<String>,
    pub body: String,
    pub span: TokenSpan,
}

/// Registry that tracks all discovered macro definitions.
#[derive(Debug, Default, Clone)]
pub struct MacroRegistry {
    definitions: BTreeMap<String, MacroDefinition>,
}

impl MacroRegistry {
    pub fn new() -> Self {
        Self {
            definitions: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&MacroDefinition> {
        self.definitions.get(name)
    }

    pub fn definitions(&self) -> impl Iterator<Item = (&String, &MacroDefinition)> {
        self.definitions.iter()
    }

    fn insert(&mut self, definition: MacroDefinition) -> Result<(), MacroError> {
        if self.definitions.contains_key(&definition.name) {
            return Err(MacroError::DuplicateDefinition {
                name: definition.name,
                span: definition.span,
            });
        }
        self.definitions.insert(definition.name.clone(), definition);
        Ok(())
    }
}

/// Errors surfaced while parsing macro definitions or populating the registry.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MacroError {
    #[error("invalid macro definition `{text}`: {reason}")]
    InvalidDefinition { text: String, reason: String },
    #[error("duplicate macro `{name}` at span {span:?}")]
    DuplicateDefinition { name: String, span: TokenSpan },
}

/// Traverse the AST and collect all macro definitions into a registry.
pub fn collect_macros(items: &[DtItem]) -> Result<MacroRegistry, MacroError> {
    let mut registry = MacroRegistry::new();
    collect_into(items, &mut registry)?;
    Ok(registry)
}

fn collect_into(items: &[DtItem], registry: &mut MacroRegistry) -> Result<(), MacroError> {
    for item in items {
        match item {
            DtItem::Macro(mac) => {
                let definition = parse_macro_definition(mac)?;
                registry.insert(definition)?;
            }
            DtItem::Node(node) => {
                collect_into(&node.children, registry)?;
            }
            DtItem::Conditional(cond) => collect_conditional(cond, registry)?,
            _ => {}
        }
    }
    Ok(())
}

fn collect_conditional(
    cond: &DtConditional,
    registry: &mut MacroRegistry,
) -> Result<(), MacroError> {
    for branch in &cond.branches {
        collect_into(&branch.items, registry)?;
    }
    Ok(())
}

fn parse_macro_definition(mac: &DtMacro) -> Result<MacroDefinition, MacroError> {
    let trimmed = mac.text.trim();
    let rest = trimmed
        .strip_prefix("#define")
        .ok_or_else(|| MacroError::InvalidDefinition {
            text: mac.text.clone(),
            reason: "missing #define prefix".into(),
        })?
        .trim_start();
    if rest.is_empty() {
        return Err(MacroError::InvalidDefinition {
            text: mac.text.clone(),
            reason: "missing macro name".into(),
        });
    }

    let (name_part, remainder) = split_name(rest);
    if name_part.is_empty() {
        return Err(MacroError::InvalidDefinition {
            text: mac.text.clone(),
            reason: "missing macro name".into(),
        });
    }

    let (args, body_part) =
        parse_args_and_body(remainder).map_err(|reason| MacroError::InvalidDefinition {
            text: mac.text.clone(),
            reason,
        })?;
    if body_part.trim().is_empty() {
        return Err(MacroError::InvalidDefinition {
            text: mac.text.clone(),
            reason: "missing macro body".into(),
        });
    }

    Ok(MacroDefinition {
        name: name_part.to_string(),
        args,
        body: body_part.trim().to_string(),
        span: mac.span,
    })
}

fn split_name(rest: &str) -> (&str, &str) {
    let mut idx = 0;
    for ch in rest.chars() {
        if ch.is_whitespace() || ch == '(' {
            break;
        }
        idx += ch.len_utf8();
    }
    rest.split_at(idx)
}

fn parse_args_and_body(rest: &str) -> Result<(Vec<String>, &str), String> {
    let mut remainder = rest.trim_start();
    let mut args = Vec::new();
    if remainder.starts_with('(') {
        let end = remainder
            .find(')')
            .ok_or_else(|| "unterminated macro argument list".to_string())?;
        let args_str = &remainder[1..end];
        args = args_str
            .split(',')
            .map(|arg| arg.trim().to_string())
            .filter(|arg| !arg.is_empty())
            .collect();
        remainder = &remainder[end + 1..];
    }
    Ok((args, remainder.trim_start()))
}
