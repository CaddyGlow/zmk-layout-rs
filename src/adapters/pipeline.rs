use std::{fs, path::PathBuf};

use serde_json::Value as JsonValue;

use crate::dts::DtsDocument;

use super::standard::AdapterError;
use super::standard::AdapterLayout;

/// Unified input sources for adapter operations (JSON or rendered DTS).
#[derive(Debug, Clone)]
pub enum LayoutInput {
    JsonPath(PathBuf),
    JsonText(String),
    DtsPath(PathBuf),
    DtsText(String),
}

/// Builder for resolving adapter layouts from a variety of inputs.
#[derive(Debug, Clone)]
pub struct AdapterPipeline {
    input: LayoutInput,
}

impl AdapterPipeline {
    pub fn from_json_path(path: impl Into<PathBuf>) -> Self {
        Self {
            input: LayoutInput::JsonPath(path.into()),
        }
    }

    pub fn from_json_text(text: impl Into<String>) -> Self {
        Self {
            input: LayoutInput::JsonText(text.into()),
        }
    }

    pub fn from_dts_path(path: impl Into<PathBuf>) -> Self {
        Self {
            input: LayoutInput::DtsPath(path.into()),
        }
    }

    pub fn from_dts_text(text: impl Into<String>) -> Self {
        Self {
            input: LayoutInput::DtsText(text.into()),
        }
    }

    /// Load an `AdapterLayout` from the configured input/template combination.
    pub fn load(self) -> Result<AdapterLayout, AdapterError> {
        match self.input {
            LayoutInput::JsonPath(path) => {
                let text = fs::read_to_string(&path)?;
                AdapterLayout::from_standard_json(&text).map_err(AdapterError::from)
            }
            LayoutInput::JsonText(text) => {
                AdapterLayout::from_standard_json(&text).map_err(AdapterError::from)
            }
            LayoutInput::DtsPath(path) => {
                let rendered = fs::read_to_string(&path)?;
                Self::load_from_dts(rendered)
            }
            LayoutInput::DtsText(text) => Self::load_from_dts(text),
        }
    }

    fn load_from_dts(rendered: String) -> Result<AdapterLayout, AdapterError> {
        let doc = DtsDocument::parse_str(&rendered)?;
        Ok(AdapterLayout::from_document(&doc))
    }

    /// Convenience to parse a JSON value directly to an adapter layout.
    pub fn from_json_value(value: JsonValue) -> Result<AdapterLayout, AdapterError> {
        AdapterLayout::from_standard_json_value(value).map_err(AdapterError::from)
    }
}
