//! Macro and preprocessor helpers.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::ast::{DtConditional, DtItem, DtMacro, DtMacroCall};
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

    /// Expand the provided macro call, substituting positional arguments.
    pub fn expand_call(&self, call: &DtMacroCall) -> Result<String, MacroExpansionError> {
        let invocation = parse_macro_call(call)?;
        let definition =
            self.get(&invocation.name)
                .ok_or_else(|| MacroExpansionError::UnknownMacro {
                    name: invocation.name.clone(),
                })?;
        if definition.args.len() != invocation.args.len() {
            return Err(MacroExpansionError::ArgCountMismatch {
                name: definition.name.clone(),
                expected: definition.args.len(),
                actual: invocation.args.len(),
            });
        }

        let mut expanded = definition.body.clone();
        for (param, value) in definition.args.iter().zip(invocation.args.iter()) {
            expanded = expanded.replace(param, value);
        }
        Ok(expanded)
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

/// Errors surfaced while parsing or expanding macro calls.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MacroExpansionError {
    #[error("{reason} in macro call `{text}`")]
    InvalidCall { text: String, reason: String },
    #[error("macro `{name}` is not defined")]
    UnknownMacro { name: String },
    #[error("macro `{name}` expects {expected} args but got {actual}")]
    ArgCountMismatch {
        name: String,
        expected: usize,
        actual: usize,
    },
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

fn parse_macro_call(call: &DtMacroCall) -> Result<MacroInvocation, MacroExpansionError> {
    let trimmed = call.text.trim();
    let (name, remainder) = split_name(trimmed);
    if name.is_empty() {
        return Err(MacroExpansionError::InvalidCall {
            text: call.text.clone(),
            reason: "missing macro name".into(),
        });
    }
    let args = parse_call_args(remainder).map_err(|reason| MacroExpansionError::InvalidCall {
        text: call.text.clone(),
        reason,
    })?;
    Ok(MacroInvocation {
        name: name.to_string(),
        args,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MacroInvocation {
    name: String,
    args: Vec<String>,
}

fn parse_call_args(remainder: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut rest = remainder.trim_start();
    if !rest.starts_with('(') {
        return Ok(args);
    }
    rest = &rest[1..];
    let mut current = String::new();
    for ch in rest.chars() {
        match ch {
            ')' => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    args.push(trimmed.to_string());
                }
                return Ok(args);
            }
            ',' => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    args.push(trimmed.to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    Err("unterminated macro call".into())
}
