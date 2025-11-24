use std::fs;
use std::path::PathBuf;

use serde_json::Value as JsonValue;
use thiserror::Error;

use crate::adapters::standard::{AdapterError, AdapterLayout};
use crate::dts::DtsDocument;
use crate::keymap::KeymapDocument;
use crate::profiles::KeyboardProfileDoc;
use crate::providers::ProviderError;
use crate::serialization::SerializeError;
use crate::tokenizer::LayoutError;

/// Identifies the provenance of a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutOrigin {
    DtsFile,
    DtsText,
    Document,
    JsonFile,
    JsonText,
    Pipeline,
    Generated,
}

/// Context for template rendering/parse mode.
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    pub source: Option<String>,
    pub mode: crate::adapters::TemplateParseMode,
}

/// Unified in-memory layout representation with cached views.
#[derive(Debug, Clone)]
pub struct LayoutHandle {
    pub source_path: Option<PathBuf>,
    pub raw_text: Option<String>,
    pub preprocessed_text: Option<String>,
    pub keymap: KeymapDocument,
    pub profile: Option<KeyboardProfileDoc>,
    pub origin: LayoutOrigin,
}

#[derive(Debug, Error)]
pub enum LayoutHandleError {
    #[error(transparent)]
    Parse(#[from] LayoutError),
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Serialize(#[from] SerializeError),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Default for LayoutHandle {
    fn default() -> Self {
        Self {
            source_path: None,
            raw_text: None,
            preprocessed_text: None,
            keymap: KeymapDocument::from(AdapterLayout::default()),
            profile: None,
            origin: LayoutOrigin::Generated,
        }
    }
}

impl LayoutHandle {
    /// Build a handle from DTS text (raw).
    pub fn from_dts_text(
        text: impl Into<String>,
        origin: LayoutOrigin,
    ) -> Result<Self, LayoutHandleError> {
        let raw = text.into();
        let document = DtsDocument::parse_str(&raw)?;
        let adapter = AdapterLayout::from_document(&document);
        let keymap = KeymapDocument::from(adapter);
        Ok(Self {
            source_path: None,
            raw_text: Some(raw),
            preprocessed_text: None,
            keymap,
            profile: None,
            origin,
        })
    }

    /// Build a handle from DTS text with both raw and preprocessed bodies preserved.
    pub fn from_dts_with_preprocessed(
        path: impl Into<PathBuf>,
        raw_text: String,
        preprocessed_text: String,
    ) -> Result<Self, LayoutHandleError> {
        let document = DtsDocument::parse_str(&preprocessed_text)?;
        let adapter = AdapterLayout::from_document(&document);
        let keymap = KeymapDocument::from(adapter);
        Ok(Self {
            source_path: Some(path.into()),
            raw_text: Some(raw_text),
            preprocessed_text: Some(preprocessed_text),
            keymap,
            profile: None,
            origin: LayoutOrigin::DtsFile,
        })
    }

    /// Build a handle from a DTS file path.
    pub fn from_dts_path(path: impl Into<PathBuf>) -> Result<Self, LayoutHandleError> {
        let path_buf = path.into();
        let raw = fs::read_to_string(&path_buf)?;
        let document = DtsDocument::parse_str(&raw)?;
        let adapter = AdapterLayout::from_document(&document);
        let keymap = KeymapDocument::from(adapter);
        Ok(Self {
            source_path: Some(path_buf),
            raw_text: Some(raw),
            preprocessed_text: None,
            keymap,
            profile: None,
            origin: LayoutOrigin::DtsFile,
        })
    }

    /// Build a handle from JSON value (standard layout format).
    pub fn from_json_value(
        value: JsonValue,
        origin: LayoutOrigin,
    ) -> Result<Self, LayoutHandleError> {
        let text = serde_json::to_string(&value)?;
        Self::from_json_text(text, origin)
    }

    /// Build a handle from JSON text (standard layout format).
    pub fn from_json_text(
        text: impl Into<String>,
        origin: LayoutOrigin,
    ) -> Result<Self, LayoutHandleError> {
        let json = text.into();
        let layout = AdapterLayout::from_standard_json(&json)?;
        let keymap = KeymapDocument::from(layout);
        Ok(Self {
            source_path: None,
            raw_text: Some(json),
            preprocessed_text: None,
            keymap,
            profile: None,
            origin,
        })
    }

    /// Build a handle from a JSON file path.
    pub fn from_json_path(
        path: impl Into<PathBuf>,
        origin: LayoutOrigin,
    ) -> Result<Self, LayoutHandleError> {
        let path_buf = path.into();
        let text = fs::read_to_string(&path_buf)?;
        let mut handle = Self::from_json_text(text, origin)?;
        handle.source_path = Some(path_buf);
        Ok(handle)
    }

    /// Render the keymap back into a DTS string using the standard adapter serializer.
    pub fn render_keymap_text(&mut self) -> Result<String, LayoutHandleError> {
        let adapter: AdapterLayout = self.keymap.clone().into();
        let document = minimal_dts_document();
        let updated = adapter.apply_to_document(document)?;
        Ok(updated.to_string()?)
    }

    /// Render the adapter layout into the standard JSON format.
    pub fn render_standard_json(&mut self) -> Result<String, LayoutHandleError> {
        let adapter: AdapterLayout = self.keymap.clone().into();
        adapter.to_standard_json().map_err(Into::into)
    }

    /// Return the source text appropriate for a diff view, flagging whether
    /// the content is preprocessed.
    pub fn raw_for_diff(&self) -> (String, bool) {
        if let Some(text) = &self.preprocessed_text {
            (text.clone(), true)
        } else if let Some(raw) = &self.raw_text {
            (raw.clone(), false)
        } else {
            (String::new(), false)
        }
    }

    /// Path used as the base label for diffs.
    pub fn diff_base_path(&self) -> PathBuf {
        self.source_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("<memory>"))
    }

    /// KeymapDocument view for mutation APIs.
    pub fn as_keymap_document(&self) -> KeymapDocument {
        self.keymap.clone()
    }
}

fn minimal_dts_document() -> DtsDocument {
    use crate::ast::{DtItem, DtNode, DtProperty, DtValue};
    use crate::tokenizer::TokenSpan;
    let empty_span = || TokenSpan::new(0, 0, 1, 1, 1, 1);
    let keymap = DtNode {
        name: "keymap".to_string(),
        raw_name: String::new(),
        span: empty_span(),
        properties: vec![DtProperty {
            name: "compatible".to_string(),
            raw_name: String::new(),
            value: DtValue {
                raw: "\"zmk,keymap\"".to_string(),
                span: empty_span(),
            },
            span: empty_span(),
            leading_comments: Vec::new(),
            trailing_comment: None,
        }],
        children: Vec::new(),
        leading_comments: Vec::new(),
        trailing_comments: Vec::new(),
    };
    let behaviors = DtNode {
        name: "behaviors".to_string(),
        raw_name: String::new(),
        span: empty_span(),
        properties: Vec::new(),
        children: Vec::new(),
        leading_comments: Vec::new(),
        trailing_comments: Vec::new(),
    };
    let macros = DtNode {
        name: "macros".to_string(),
        raw_name: String::new(),
        span: empty_span(),
        properties: Vec::new(),
        children: Vec::new(),
        leading_comments: Vec::new(),
        trailing_comments: Vec::new(),
    };
    let combos = DtNode {
        name: "combos".to_string(),
        raw_name: String::new(),
        span: empty_span(),
        properties: Vec::new(),
        children: Vec::new(),
        leading_comments: Vec::new(),
        trailing_comments: Vec::new(),
    };
    DtsDocument::from_items(vec![
        DtItem::Node(behaviors),
        DtItem::Node(macros),
        DtItem::Node(combos),
        DtItem::Node(keymap),
    ])
}
