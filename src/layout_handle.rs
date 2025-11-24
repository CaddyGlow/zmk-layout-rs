use std::fs;
use std::path::PathBuf;

use serde_json::Value as JsonValue;
use thiserror::Error;

use crate::adapters::standard::{
    AdapterError, AdapterLayout, TemplateError, TemplateParseMode, render_layout_with_template,
};
use crate::dts::DtsDocument;
use crate::profiles::KeyboardProfileDoc;
use crate::providers::KeymapDocument;
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
    pub mode: TemplateParseMode,
}

/// Unified in-memory layout representation with cached views.
#[derive(Debug, Clone)]
pub struct LayoutHandle {
    pub source_path: Option<PathBuf>,
    pub raw_text: Option<String>,
    pub preprocessed_text: Option<String>,
    pub document: DtsDocument,
    pub adapter_layout: Option<AdapterLayout>,
    pub profile: Option<KeyboardProfileDoc>,
    pub template_source: Option<String>,
    pub template_mode: TemplateParseMode,
    pub origin: LayoutOrigin,
}

#[derive(Debug, Error)]
pub enum LayoutHandleError {
    #[error(transparent)]
    Parse(#[from] LayoutError),
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error(transparent)]
    Serialize(#[from] SerializeError),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error("template source required to build a Devicetree document from JSON")]
    MissingTemplate,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Default for LayoutHandle {
    fn default() -> Self {
        Self {
            source_path: None,
            raw_text: None,
            preprocessed_text: None,
            document: DtsDocument::from_items(Vec::new()),
            adapter_layout: None,
            profile: None,
            template_source: None,
            template_mode: TemplateParseMode::default(),
            origin: LayoutOrigin::Generated,
        }
    }
}

impl LayoutHandle {
    /// Build a handle from an adapter layout using the provided template context.
    pub fn from_adapter_layout(
        adapter_layout: AdapterLayout,
        template_ctx: TemplateContext,
        source_path: Option<PathBuf>,
        origin: LayoutOrigin,
    ) -> Result<Self, LayoutHandleError> {
        let template = template_ctx
            .source
            .as_deref()
            .ok_or(LayoutHandleError::MissingTemplate)?;
        let rendered = render_layout_with_template(&adapter_layout, template)?;
        let document = DtsDocument::parse_str(&rendered)?;
        Ok(Self {
            source_path,
            raw_text: Some(rendered),
            preprocessed_text: None,
            document,
            adapter_layout: Some(adapter_layout),
            profile: None,
            template_source: Some(template.to_string()),
            template_mode: template_ctx.mode,
            origin,
        })
    }

    /// Build a handle directly from a parsed document, preserving optional raw text.
    pub fn from_document(
        document: DtsDocument,
        raw_text: Option<String>,
        origin: LayoutOrigin,
    ) -> Self {
        Self {
            source_path: None,
            raw_text,
            preprocessed_text: None,
            document,
            adapter_layout: None,
            profile: None,
            template_source: None,
            template_mode: TemplateParseMode::default(),
            origin,
        }
    }

    /// Build a handle from DTS text (raw).
    pub fn from_dts_text(
        text: impl Into<String>,
        origin: LayoutOrigin,
    ) -> Result<Self, LayoutHandleError> {
        let raw = text.into();
        let document = DtsDocument::parse_str(&raw)?;
        Ok(Self {
            source_path: None,
            raw_text: Some(raw),
            preprocessed_text: None,
            document,
            adapter_layout: None,
            profile: None,
            template_source: None,
            template_mode: TemplateParseMode::default(),
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
        Ok(Self {
            source_path: Some(path.into()),
            raw_text: Some(raw_text),
            preprocessed_text: Some(preprocessed_text),
            document,
            adapter_layout: None,
            profile: None,
            template_source: None,
            template_mode: TemplateParseMode::default(),
            origin: LayoutOrigin::DtsFile,
        })
    }

    /// Build a handle from a DTS file path.
    pub fn from_dts_path(path: impl Into<PathBuf>) -> Result<Self, LayoutHandleError> {
        let path_buf = path.into();
        let raw = fs::read_to_string(&path_buf)?;
        let document = DtsDocument::parse_str(&raw)?;
        Ok(Self {
            source_path: Some(path_buf),
            raw_text: Some(raw),
            preprocessed_text: None,
            document,
            adapter_layout: None,
            profile: None,
            template_source: None,
            template_mode: TemplateParseMode::default(),
            origin: LayoutOrigin::DtsFile,
        })
    }

    /// Build a handle from JSON value using a template to render the DTS.
    pub fn from_json_value(
        value: JsonValue,
        template_ctx: TemplateContext,
        origin: LayoutOrigin,
    ) -> Result<Self, LayoutHandleError> {
        let text = serde_json::to_string(&value)?;
        Self::from_json_text(text, template_ctx, origin)
    }

    /// Build a handle from JSON text using a template to render the DTS.
    pub fn from_json_text(
        text: impl Into<String>,
        template_ctx: TemplateContext,
        origin: LayoutOrigin,
    ) -> Result<Self, LayoutHandleError> {
        let json_text = text.into();
        let adapter_layout = AdapterLayout::from_standard_json(&json_text)?;
        let template = template_ctx
            .source
            .as_deref()
            .ok_or(LayoutHandleError::MissingTemplate)?;
        let rendered = render_layout_with_template(&adapter_layout, template)?;
        let document = DtsDocument::parse_str(&rendered)?;
        Ok(Self {
            source_path: None,
            raw_text: Some(rendered),
            preprocessed_text: None,
            document,
            adapter_layout: Some(adapter_layout),
            profile: None,
            template_source: Some(template.to_string()),
            template_mode: template_ctx.mode,
            origin,
        })
    }

    /// Build a handle from a JSON file using a template to render the DTS.
    pub fn from_json_path(
        path: impl Into<PathBuf>,
        template_ctx: TemplateContext,
    ) -> Result<Self, LayoutHandleError> {
        let path_buf = path.into();
        let text = fs::read_to_string(&path_buf)?;
        let mut handle = Self::from_json_text(text, template_ctx, LayoutOrigin::JsonFile)?;
        handle.source_path = Some(path_buf);
        Ok(handle)
    }

    /// Construct an empty handle based on a profile template or a minimal fallback.
    pub fn empty(profile: &KeyboardProfileDoc) -> Result<Self, LayoutHandleError> {
        let template_path = PathBuf::from(&profile.layout.template);
        let template_source = fs::read_to_string(&template_path).ok();
        let mut adapter_layout = AdapterLayout::default();
        adapter_layout.metadata.title = Some(profile.metadata.name.clone());
        adapter_layout.metadata.author = Some(profile.metadata.vendor.clone());

        if let Some(template) = template_source.as_deref() {
            let rendered = render_layout_with_template(&adapter_layout, template)?;
            let document = DtsDocument::parse_str(&rendered)?;
            return Ok(Self {
                source_path: None,
                raw_text: Some(rendered),
                preprocessed_text: None,
                document,
                adapter_layout: Some(adapter_layout),
                profile: Some(profile.clone()),
                template_source: Some(template.to_string()),
                template_mode: TemplateParseMode::default(),
                origin: LayoutOrigin::Generated,
            });
        }

        // Fallback to a minimal skeleton when the template cannot be resolved.
        let minimal = r#"
/ {
    behaviors {};
    macros {};
    combos {};
};

        keymap {
            compatible = "zmk,keymap";
            base {
                bindings = < &none >;
            };
        };
"#
        .to_string();
        let document = DtsDocument::parse_str(&minimal)?;
        let adapter_layout = AdapterLayout::from_document(&document);
        Ok(Self {
            source_path: None,
            raw_text: Some(minimal),
            preprocessed_text: None,
            document,
            adapter_layout: Some(adapter_layout),
            profile: Some(profile.clone()),
            template_source: None,
            template_mode: TemplateParseMode::default(),
            origin: LayoutOrigin::Generated,
        })
    }

    /// Return text to use for diffs and whether it reflects preprocessed content.
    pub fn raw_for_diff(&self) -> (String, bool) {
        if let Some(pre) = &self.preprocessed_text {
            return (pre.clone(), true);
        }
        if let Some(raw) = &self.raw_text {
            return (raw.clone(), false);
        }
        // Fallback to serialized document if no stored text is available.
        let serialized = self.document.to_string().unwrap_or_default();
        (serialized, false)
    }

    /// Cached view of the adapter layout.
    pub fn as_adapter_layout(&mut self) -> AdapterLayout {
        if let Some(layout) = &self.adapter_layout {
            return layout.clone();
        }
        let layout = AdapterLayout::from_document(&self.document);
        self.adapter_layout = Some(layout.clone());
        layout
    }

    /// KeymapDocument view for provider APIs.
    pub fn as_keymap_document(&self) -> KeymapDocument {
        KeymapDocument::from_document(self.document.clone())
    }

    /// Render the Devicetree text from the parsed document.
    pub fn render_keymap_text(&self) -> Result<String, LayoutHandleError> {
        Ok(self.document.to_string()?)
    }

    /// Render the standard JSON view.
    pub fn render_standard_json(&mut self) -> Result<String, LayoutHandleError> {
        Ok(self.as_adapter_layout().to_standard_json()?)
    }

    /// Path where the layout originated, if any.
    pub fn source_path(&self) -> Option<&PathBuf> {
        self.source_path.as_ref()
    }

    /// Best-effort path for diagnostics/diffs.
    pub fn diff_base_path(&self) -> PathBuf {
        self.source_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("<memory>"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::standard::TemplateParseMode;

    #[test]
    fn roundtrip_dts_to_json_and_back() {
        let dts = r#"
/ {
    behaviors {};
    macros {};
    combos {};
};

keymap {
    compatible = "zmk,keymap";
    base {
        bindings = < &none >;
    };
};
"#;
        let mut handle =
            LayoutHandle::from_dts_text(dts, LayoutOrigin::DtsText).expect("parse dts");
        let json = handle.render_standard_json().expect("to json");
        let template_ctx = TemplateContext {
            source: Some(dts.to_string()),
            mode: TemplateParseMode::FullDocument,
        };
        let rebuilt = LayoutHandle::from_json_text(json, template_ctx, LayoutOrigin::JsonText)
            .expect("from json");
        assert_eq!(handle.as_adapter_layout(), rebuilt.adapter_layout.unwrap());
    }

    #[test]
    fn raw_for_diff_prefers_preprocessed() {
        let raw = "/ { behaviors {}; };";
        let pre = "/ { behaviors {}; macros {}; };";
        let handle = LayoutHandle::from_dts_with_preprocessed(
            PathBuf::from("keymap.dtsi"),
            raw.into(),
            pre.into(),
        )
        .expect("build handle");
        let (text, is_preprocessed) = handle.raw_for_diff();
        assert!(is_preprocessed);
        assert_eq!(text, "/ { behaviors {}; macros {}; };");
        assert_eq!(handle.diff_base_path(), PathBuf::from("keymap.dtsi"));
    }
}
