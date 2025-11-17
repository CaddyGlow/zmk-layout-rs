//! Walks the AST and emits Devicetree text.

use crate::ast::{
    DtComment, DtConditional, DtInclude, DtItem, DtMacro, DtMacroCall, DtNode, DtProperty,
    DtTemplate,
};
use crate::tokenizer::TokenSpan;
use thiserror::Error;

/// Configuration for serialization.
#[derive(Clone, Debug)]
pub struct SerializeConfig {
    pub indent: &'static str,
}

impl Default for SerializeConfig {
    fn default() -> Self {
        Self { indent: "  " }
    }
}

/// Error surfaced when serialization fails due to incomplete AST data.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SerializeError {
    #[error("property `{name}` is missing a value at {span:?}")]
    MissingValue { name: String, span: TokenSpan },
}

/// Serialize the given AST items using the default configuration.
pub fn serialize(items: &[DtItem]) -> Result<String, SerializeError> {
    serialize_with_config(items, SerializeConfig::default())
}

/// Serialize with the provided configuration.
pub fn serialize_with_config(
    items: &[DtItem],
    config: SerializeConfig,
) -> Result<String, SerializeError> {
    let mut writer = Serializer::new(config);
    writer.write_items(items, 0)?;
    Ok(writer.finish())
}

struct Serializer {
    config: SerializeConfig,
    output: String,
}

impl Serializer {
    fn new(config: SerializeConfig) -> Self {
        Self {
            config,
            output: String::new(),
        }
    }

    fn finish(mut self) -> String {
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.output
    }

    fn write_items(&mut self, items: &[DtItem], indent: usize) -> Result<(), SerializeError> {
        for item in items {
            self.write_item(item, indent)?;
        }
        Ok(())
    }

    fn write_item(&mut self, item: &DtItem, indent: usize) -> Result<(), SerializeError> {
        match item {
            DtItem::Node(node) => self.write_node(node, indent),
            DtItem::Conditional(cond) => self.write_conditional(cond, indent),
            DtItem::Macro(mac) => self.write_macro(mac, indent),
            DtItem::MacroCall(call) => self.write_macro_call(call, indent),
            DtItem::Include(include) => self.write_include(include, indent),
            DtItem::Template(template) => self.write_template(template, indent),
            DtItem::Comment(comment) => {
                self.write_comments(std::slice::from_ref(comment), indent);
                Ok(())
            }
        }
    }

    fn write_node(&mut self, node: &DtNode, indent: usize) -> Result<(), SerializeError> {
        self.write_comments(&node.leading_comments, indent);
        let name = if node.raw_name.is_empty() {
            node.name.as_str()
        } else {
            node.raw_name.as_str()
        };
        self.push_indent(indent);
        self.output.push_str(name);
        self.output.push_str(" {\n");

        for prop in &node.properties {
            self.write_property(prop, indent + 1)?;
        }
        for child in &node.children {
            self.write_item(child, indent + 1)?;
        }

        self.push_indent(indent);
        self.output.push_str("};\n");
        self.write_comments(&node.trailing_comments, indent);
        Ok(())
    }

    fn write_property(&mut self, prop: &DtProperty, indent: usize) -> Result<(), SerializeError> {
        self.write_comments(&prop.leading_comments, indent);
        if prop.value.raw.trim().is_empty() {
            return Err(SerializeError::MissingValue {
                name: prop.name.clone(),
                span: prop.span,
            });
        }

        let name = if prop.raw_name.is_empty() {
            prop.name.as_str()
        } else {
            prop.raw_name.as_str()
        };
        self.push_indent(indent);
        self.output.push_str(name);
        self.output.push_str(" = ");
        self.output.push_str(&prop.value.raw);
        self.output.push(';');
        if let Some(comment) = &prop.trailing_comment {
            let starts_with_ws = comment
                .text
                .chars()
                .next()
                .map(|ch| ch.is_whitespace())
                .unwrap_or(false);
            if starts_with_ws {
                self.output.push_str(&comment.text);
            } else {
                self.output.push(' ');
                self.output.push_str(&comment.text);
            }
        }
        self.output.push('\n');
        Ok(())
    }

    fn write_conditional(
        &mut self,
        cond: &DtConditional,
        indent: usize,
    ) -> Result<(), SerializeError> {
        for branch in &cond.branches {
            self.write_comments(&branch.directive.leading_comments, indent);
            self.push_indent(indent);
            self.output.push_str(&branch.directive.text);
            self.output.push('\n');
            self.write_items(&branch.items, indent)?;
        }
        self.write_comments(&cond.end_directive.leading_comments, indent);
        self.push_indent(indent);
        self.output.push_str(&cond.end_directive.text);
        self.output.push('\n');
        Ok(())
    }

    fn write_macro(&mut self, mac: &DtMacro, indent: usize) -> Result<(), SerializeError> {
        self.write_comments(&mac.leading_comments, indent);
        self.push_indent(indent);
        self.output.push_str(&mac.text);
        self.output.push('\n');
        Ok(())
    }

    fn write_macro_call(
        &mut self,
        call: &DtMacroCall,
        indent: usize,
    ) -> Result<(), SerializeError> {
        self.write_comments(&call.leading_comments, indent);
        self.push_indent(indent);
        self.output.push_str(&call.text);
        self.output.push('\n');
        Ok(())
    }

    fn write_include(&mut self, include: &DtInclude, indent: usize) -> Result<(), SerializeError> {
        self.write_comments(&include.leading_comments, indent);
        self.push_indent(indent);
        self.output.push_str(&include.text);
        self.output.push('\n');
        Ok(())
    }

    fn write_template(
        &mut self,
        template: &DtTemplate,
        indent: usize,
    ) -> Result<(), SerializeError> {
        self.write_comments(&template.leading_comments, indent);
        self.push_indent(indent);
        self.output.push_str(&template.raw);
        self.output.push('\n');
        Ok(())
    }

    fn write_comments(&mut self, comments: &[DtComment], indent: usize) {
        for comment in comments {
            self.push_indent(indent);
            self.output.push_str(&comment.text);
            self.output.push('\n');
        }
    }

    fn push_indent(&mut self, indent: usize) {
        for _ in 0..indent {
            self.output.push_str(self.config.indent);
        }
    }
}
