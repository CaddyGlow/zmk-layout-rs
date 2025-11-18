use std::collections::BTreeMap;

use serde_json::Value;
use thiserror::Error;

use super::layout::AdapterLayout;

pub fn template_contains_placeholders(source: &str) -> bool {
    source.contains("{{") || source.contains("{%")
}

pub(crate) fn merge_template_metadata(
    layout: &mut AdapterLayout,
    template_source: &str,
    rendered_source: &str,
) -> Result<(), TemplateError> {
    let captured = capture_template_values(template_source, rendered_source)?;
    apply_captured_template_values(layout, captured);
    Ok(())
}

fn apply_captured_template_values(layout: &mut AdapterLayout, captured: BTreeMap<String, String>) {
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

fn capture_template_values(
    template_source: &str,
    rendered_source: &str,
) -> Result<BTreeMap<String, String>, TemplateError> {
    let segments = parse_template_segments(template_source)?;
    let mut cursor = 0;
    let mut pending_placeholder: Option<String> = None;
    let mut captures = BTreeMap::new();

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
                        captures.insert(name, value.to_string());
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
                    captures.insert(previous, String::new());
                }
            }
        }
    }

    if let Some(name) = pending_placeholder {
        captures.insert(name, rendered_source[cursor..].to_string());
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

        let rendered = render_layout_with_template(&layout, template);
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

        let rendered = render_layout_with_template(&layout, template);
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
