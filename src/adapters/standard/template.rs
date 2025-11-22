use std::collections::BTreeMap;

use minijinja::Error as MiniJinjaError;
use regex::Regex;
use serde_json::Value;
use thiserror::Error;

use super::layout::AdapterLayout;

#[derive(Debug, Default)]
pub(crate) struct TemplateCapture {
    pub values: BTreeMap<String, String>,
    pub fragments: Vec<CapturedFragment>,
}

#[derive(Debug, Clone)]
pub(crate) struct CapturedFragment {
    pub key: String,
    pub start: usize,
    pub end: usize,
}

pub fn template_contains_placeholders(source: &str) -> bool {
    source.contains("{{") || source.contains("{%")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateParseMode {
    StripPlaceholders,
    FullDocument,
}

/// Captures a user-defined slice of the rendered DTS using regex delimiters.
#[derive(Debug, Clone)]
pub struct RegexExtractionConfig {
    pub placeholder: String,
    pub start_delimiter: Regex,
    pub end_delimiter: Regex,
    pub strip_for_parse: bool,
}

impl RegexExtractionConfig {
    pub fn new(
        placeholder: impl Into<String>,
        start_delimiter: &str,
        end_delimiter: &str,
    ) -> Result<Self, regex::Error> {
        Ok(Self {
            placeholder: placeholder.into(),
            start_delimiter: Regex::new(start_delimiter)?,
            end_delimiter: Regex::new(end_delimiter)?,
            strip_for_parse: false,
        })
    }

    pub fn with_strip_for_parse(mut self, strip: bool) -> Self {
        self.strip_for_parse = strip;
        self
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn merge_template_metadata(
    layout: &mut AdapterLayout,
    template_source: &str,
    rendered_source: &str,
) -> Result<(), TemplateError> {
    let TemplateCapture { values, .. } = capture_template_values(template_source, rendered_source)?;
    apply_captured_template_values(layout, values);
    Ok(())
}

pub(crate) fn apply_captured_template_values(
    layout: &mut AdapterLayout,
    captured: BTreeMap<String, String>,
) {
    const DERIVED_KEYS: &[&str] = &[
        "layer_names_defines",
        "layer_defines",
        "rendered_layers",
        "keymap_node",
        "macros",
        "user_macros_dtsi",
        "behaviors",
        "user_behaviors_dtsi",
        "combos",
        "combos_dtsi",
    ];

    for (key, value) in captured {
        if DERIVED_KEYS.contains(&key.as_str()) {
            continue;
        }
        if key == "keyboard_name" {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                layout.metadata.title = Some(trimmed.to_string());
            }
        }
        layout.metadata.extras.insert(key, Value::String(value));
    }
}

pub(crate) fn capture_template_sections(
    template_source: &str,
    rendered_source: &str,
) -> Result<TemplateCapture, TemplateError> {
    capture_template_values(template_source, rendered_source)
}

pub(crate) fn capture_regex_sections(
    rendered_source: &str,
    configs: &[RegexExtractionConfig],
) -> TemplateCapture {
    let mut capture = TemplateCapture::default();
    for config in configs {
        let Some(start) = config.start_delimiter.find(rendered_source) else {
            continue;
        };
        let haystack = &rendered_source[start.end()..];
        let Some(end) = config.end_delimiter.find(haystack) else {
            continue;
        };
        let value_start = start.end();
        let value_end = value_start + end.start();
        capture.values.insert(
            config.placeholder.clone(),
            rendered_source[value_start..value_end].to_string(),
        );
        capture.fragments.push(CapturedFragment {
            key: config.placeholder.clone(),
            start: value_start,
            end: value_end,
        });
    }
    capture.fragments.sort_by_key(|frag| frag.start);
    capture
}

pub(crate) fn strip_template_fragments(
    rendered_source: &str,
    fragments: &[CapturedFragment],
) -> String {
    strip_fragments_matching(rendered_source, fragments, should_strip_before_parse)
}

pub(crate) fn strip_fragments_matching(
    rendered_source: &str,
    fragments: &[CapturedFragment],
    should_strip: impl Fn(&str) -> bool,
) -> String {
    let mut output = String::with_capacity(rendered_source.len());
    let mut cursor = 0;
    for fragment in fragments {
        if should_strip(&fragment.key) {
            if cursor < fragment.start {
                output.push_str(&rendered_source[cursor..fragment.start]);
            }
            cursor = fragment.end;
        }
    }
    output.push_str(&rendered_source[cursor..]);
    output
}

fn should_strip_before_parse(key: &str) -> bool {
    matches!(
        key,
        "includes"
            | "resolved_includes"
            | "custom_devicetree"
            | "input_listeners"
            | "input_listeners_dtsi"
            | "custom_defined_behaviors"
            | "input_processors"
            | "system_behaviors_dts"
            | "key_position_header"
            | "custom_defined_macros"
    )
}

fn capture_template_values(
    template_source: &str,
    rendered_source: &str,
) -> Result<TemplateCapture, TemplateError> {
    let segments = parse_template_segments(template_source)?;
    let mut cursor = 0;
    let mut pending_placeholder: Option<String> = None;
    let mut captures = TemplateCapture::default();

    for segment in segments {
        match segment {
            TemplateSegment::Literal(literal) => {
                if literal.is_empty() {
                    continue;
                }
                let haystack = &rendered_source[cursor..];
                if let Some(offset) = haystack.find(&literal) {
                    let absolute = cursor + offset;
                    if let Some(name) = pending_placeholder.take() {
                        let value = &rendered_source[cursor..absolute];
                        captures.values.insert(name.clone(), value.to_string());
                        captures.fragments.push(CapturedFragment {
                            key: name,
                            start: cursor,
                            end: absolute,
                        });
                    } else if absolute != cursor {
                        return Err(TemplateError::UnexpectedContent {
                            literal: snippet(&literal),
                            context: snippet(&rendered_source[cursor..absolute]),
                        });
                    }
                    cursor = absolute + literal.len();
                } else {
                    return Err(TemplateError::LiteralNotFound {
                        literal: snippet(&literal),
                    });
                }
            }
            TemplateSegment::Placeholder(name) => {
                if let Some(previous) = pending_placeholder.replace(name.clone()) {
                    captures.values.insert(previous.clone(), String::new());
                    captures.fragments.push(CapturedFragment {
                        key: previous,
                        start: cursor,
                        end: cursor,
                    });
                }
            }
        }
    }

    if let Some(name) = pending_placeholder {
        captures
            .values
            .insert(name.clone(), rendered_source[cursor..].to_string());
        captures.fragments.push(CapturedFragment {
            key: name,
            start: cursor,
            end: rendered_source.len(),
        });
    } else if cursor != rendered_source.len() {
        return Err(TemplateError::TrailingContent {
            trailing: snippet(&rendered_source[cursor..]),
        });
    }

    Ok(captures)
}

fn parse_template_segments(source: &str) -> Result<Vec<TemplateSegment>, TemplateError> {
    let mut segments = Vec::new();
    let mut cursor = 0;
    while let Some(start) = source[cursor..].find("{{") {
        let absolute_start = cursor + start;
        if absolute_start > cursor {
            segments.push(TemplateSegment::Literal(
                source[cursor..absolute_start].to_string(),
            ));
        }
        let after_start = absolute_start + 2;
        let end = source[after_start..]
            .find("}}")
            .map(|offset| after_start + offset)
            .ok_or(TemplateError::UnterminatedPlaceholder {
                offset: absolute_start,
            })?;
        let raw_name = &source[after_start..end];
        let name = canonical_placeholder_name(raw_name);
        segments.push(TemplateSegment::Placeholder(name));
        cursor = end + 2;
    }
    if cursor < source.len() {
        segments.push(TemplateSegment::Literal(source[cursor..].to_string()));
    }
    if segments.is_empty() {
        segments.push(TemplateSegment::Literal(String::new()));
    }
    Ok(segments)
}

fn canonical_placeholder_name(raw: &str) -> String {
    let normalized: String = raw.split_whitespace().collect();
    normalized
        .strip_prefix("content.")
        .map(|value| value.to_string())
        .unwrap_or(normalized)
}

fn snippet(text: &str) -> String {
    let mut sanitized = text.replace('\n', "\\n");
    if sanitized.len() > 40 {
        sanitized.truncate(40);
        sanitized.push('…');
    }
    sanitized
}

#[derive(Debug, Clone)]
enum TemplateSegment {
    Literal(String),
    Placeholder(String),
}

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("failed to parse template: missing closing '}}' for placeholder at byte {offset}")]
    UnterminatedPlaceholder { offset: usize },
    #[error("rendered DTS is missing literal `{literal}` from the template")]
    LiteralNotFound { literal: String },
    #[error("rendered DTS contains unexpected content before literal `{literal}`: `{context}`")]
    UnexpectedContent { literal: String, context: String },
    #[error("rendered DTS has trailing content outside the template: `{trailing}`")]
    TrailingContent { trailing: String },
    #[error("failed to build template context: {0}")]
    Context(#[from] serde_json::Error),
    #[error(transparent)]
    Render(#[from] MiniJinjaError),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        layout::AdapterLayout,
        render::render_layout_with_template,
        types::{LayerSpec, LayoutMetadata},
    };
    use super::*;
    use crate::dts::DtsDocument;

    #[test]
    fn captures_metadata_from_moergo_template() {
        let template = include_str!("../../../examples/moergo_glove80.j2");

        let mut layout = AdapterLayout {
            layers: vec![LayerSpec {
                name: "base".into(),
                bindings: vec!["&kp A".into(), "&kp B".into()],
            }],
            combos: Vec::new(),
            behaviors: Vec::new(),
            macros: Vec::new(),
            input_listeners: Vec::new(),
            metadata: LayoutMetadata::default(),
        };
        layout.metadata.title = Some("Glove80".into());
        layout.metadata.extras.insert(
            "includes".into(),
            json!("#include <behaviors.dtsi>\n#include <dt-bindings/zmk/keys.h>"),
        );
        layout.metadata.extras.insert(
            "custom_devicetree".into(),
            json!("&sensor {\n    status = \"okay\";\n};\n"),
        );
        layout
            .metadata
            .extras
            .insert("custom_defined_behaviors".into(), json!("/* custom */\n"));

        let rendered =
            render_layout_with_template(&layout, template).expect("template should render");
        let document = DtsDocument::parse_str(&rendered).expect("template renders valid DTS");
        let mut extracted = AdapterLayout::from_document(&document);
        merge_template_metadata(&mut extracted, template, &rendered)
            .expect("metadata extraction succeeds");

        let includes = extracted
            .metadata
            .extras
            .get("includes")
            .and_then(|value| value.as_str())
            .expect("includes captured");
        assert!(includes.contains("#include <behaviors.dtsi>"));

        let custom_dt = extracted
            .metadata
            .extras
            .get("custom_devicetree")
            .and_then(|value| value.as_str())
            .expect("custom devicetree captured");
        assert!(custom_dt.contains("&sensor"));

        assert_eq!(
            extracted.metadata.title.as_deref(),
            Some("Glove80"),
            "keyboard_name placeholder hydrates title"
        );
    }

    #[test]
    fn export_captures_include_statements() {
        let source = r#"
#include <behaviors.dtsi>
#include <dt-bindings/zmk/outputs.h>

/* On demand includes */
#include <dt-bindings/zmk/input_transform.h>
#include <input/processors.dtsi>

/ {
    keymap {};
};
"#;

        let doc = DtsDocument::parse_str(source).expect("valid DTS with includes");
        let layout = AdapterLayout::from_document(&doc);

        let includes = layout
            .metadata
            .extras
            .get("resolved_includes")
            .and_then(|value| value.as_str())
            .expect("resolved includes captured");
        assert!(includes.contains("#include <behaviors.dtsi>"));
        assert!(includes.contains("#include <input/processors.dtsi>"));
    }

    #[test]
    fn template_deduplicates_static_include_lines() {
        let mut layout = AdapterLayout {
            layers: vec![LayerSpec {
                name: "base".into(),
                bindings: vec!["&kp A".into()],
            }],
            combos: Vec::new(),
            behaviors: Vec::new(),
            macros: Vec::new(),
            input_listeners: Vec::new(),
            metadata: LayoutMetadata::default(),
        };
        layout.metadata.extras.insert(
            "resolved_includes".into(),
            json!("#include <behaviors.dtsi>\n#include <dt-bindings/zmk/input_transform.h>"),
        );

        let template = r#"
#include <behaviors.dtsi>
{{includes}}

{{keymap_node}}
"#;

        let rendered =
            render_layout_with_template(&layout, template).expect("template should render");
        DtsDocument::parse_str(&rendered).expect("template output should be valid DTS");

        assert_eq!(
            rendered.matches("#include <behaviors.dtsi>").count(),
            1,
            "static include should not be duplicated",
        );
        assert!(
            rendered.contains("#include <dt-bindings/zmk/input_transform.h>"),
            "custom include should remain"
        );
    }
}
