//! High-level DTS helpers that wrap the tokenizer/parser/serializer stack.

use std::{fs, path::Path};

use thiserror::Error;

use crate::{
    ast::DtItem,
    parser::parse_layout,
    serialization::{SerializeConfig, SerializeError, serialize, serialize_with_config},
    tokenizer::LayoutError,
};

/// Errors that can surface when loading or saving DTS documents.
#[derive(Debug, Error)]
pub enum DtsError {
    #[error(transparent)]
    Parse(#[from] LayoutError),
    #[error(transparent)]
    Serialize(#[from] SerializeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Parsed Devicetree document consisting of ordered AST items.
#[derive(Debug, Clone)]
pub struct DtsDocument {
    pub items: Vec<DtItem>,
}

impl DtsDocument {
    /// Parse the provided string into a DTS document.
    pub fn parse_str(source: &str) -> Result<Self, LayoutError> {
        let items = parse_layout(source)?;
        Ok(Self { items })
    }

    /// Parse an on-disk DTS file.
    pub fn parse_file(path: impl AsRef<Path>) -> Result<Self, DtsError> {
        let text = fs::read_to_string(path)?;
        Ok(Self::parse_str(&text)?)
    }

    /// Return a mutable reference to the document items for modification.
    pub fn items_mut(&mut self) -> &mut Vec<DtItem> {
        &mut self.items
    }

    /// Serialize the document using the default formatting configuration.
    pub fn to_string(&self) -> Result<String, SerializeError> {
        serialize(&self.items)
    }

    /// Serialize using a custom formatting configuration.
    pub fn to_string_with_config(&self, config: SerializeConfig) -> Result<String, SerializeError> {
        serialize_with_config(&self.items, config)
    }

    /// Persist the document to the provided path using the default formatting options.
    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<(), DtsError> {
        let contents = self.to_string()?;
        fs::write(path, contents)?;
        Ok(())
    }
}

/// Parse a DTS string into a [`DtsDocument`].
pub fn parse_str(source: &str) -> Result<DtsDocument, LayoutError> {
    DtsDocument::parse_str(source)
}

/// Parse a DTS file from disk.
pub fn parse_file(path: impl AsRef<Path>) -> Result<DtsDocument, DtsError> {
    DtsDocument::parse_file(path)
}
