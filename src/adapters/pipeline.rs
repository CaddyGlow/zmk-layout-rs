use std::{fs, path::PathBuf};

use serde_json::Value as JsonValue;

use crate::dts::DtsDocument;

use super::{
    standard::{
        AdapterError, AdapterLayout, TemplateParseMode, export_standard_str_with_template_mode,
    },
};

/// Unified input sources for adapter operations (JSON or rendered DTS).
#[derive(Debug, Clone)]
pub enum LayoutInput {
    JsonPath(PathBuf),
    JsonText(String),
    DtsPath(PathBuf),
    DtsText(String),
}

/// Builder for resolving adapter layouts from a variety of inputs with optional template metadata.
#[derive(Debug, Clone)]
pub struct AdapterPipeline {
    input: LayoutInput,
    template_source: Option<String>,
    template_mode: TemplateParseMode,
}

impl AdapterPipeline {
    pub fn from_json_path(path: impl Into<PathBuf>) -> Self {
        Self {
            input: LayoutInput::JsonPath(path.into()),
            template_source: None,
            template_mode: TemplateParseMode::default(),
        }
    }

    pub fn from_json_text(text: impl Into<String>) -> Self {
        Self {
            input: LayoutInput::JsonText(text.into()),
            template_source: None,
            template_mode: TemplateParseMode::default(),
        }
    }

    pub fn from_dts_path(path: impl Into<PathBuf>) -> Self {
        Self {
            input: LayoutInput::DtsPath(path.into()),
            template_source: None,
            template_mode: TemplateParseMode::default(),
        }
    }

    pub fn from_dts_text(text: impl Into<String>) -> Self {
        Self {
            input: LayoutInput::DtsText(text.into()),
            template_source: None,
            template_mode: TemplateParseMode::default(),
        }
    }

    /// Attach a template source to extract metadata/sections during export.
    pub fn template_source(mut self, template: impl Into<String>) -> Self {
        self.template_source = Some(template.into());
        self
    }

    /// Override the template parsing mode (strip placeholders vs full document).
    pub fn template_mode(mut self, mode: TemplateParseMode) -> Self {
        self.template_mode = mode;
        self
    }

    /// Load an `AdapterLayout` from the configured input/template combination.
    pub fn load(self) -> Result<AdapterLayout, AdapterError> {
        let template = self.template_source;
        let mode = self.template_mode;
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
                Self::load_from_dts(rendered, template, mode)
            }
            LayoutInput::DtsText(text) => Self::load_from_dts(text, template, mode),
        }
    }

    fn load_from_dts(
        rendered: String,
        template_source: Option<String>,
        template_mode: TemplateParseMode,
    ) -> Result<AdapterLayout, AdapterError> {
        if let Some(template) = template_source {
            let json = export_standard_str_with_template_mode(
                &rendered,
                &template,
                template_mode,
            )?;
            return AdapterLayout::from_standard_json(&json).map_err(AdapterError::from);
        }
        let doc = DtsDocument::parse_str(&rendered)?;
        Ok(AdapterLayout::from_document(&doc))
    }

    /// Convenience to parse a JSON value directly to an adapter layout.
    pub fn from_json_value(value: JsonValue) -> Result<AdapterLayout, AdapterError> {
        AdapterLayout::from_standard_json_value(value).map_err(AdapterError::from)
    }
}
