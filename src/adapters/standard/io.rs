use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    dts::{DtsDocument, DtsError},
    profiles::KeyboardProfileDoc,
};

use super::{
    AdapterError,
    layout::AdapterLayout,
    render::render_layout_with_template,
    template::{
        RegexExtractionConfig, TemplateCapture, TemplateParseMode, apply_captured_template_values,
        capture_regex_sections, capture_template_sections, strip_fragments_matching,
        strip_template_fragments, template_contains_placeholders,
    },
};

/// Export a document to the standard JSON format.
pub fn export_standard_str(document: &DtsDocument) -> Result<String, AdapterError> {
    Ok(AdapterLayout::from_document(document).to_standard_json()?)
}

/// Export a document directly to a file containing the standard JSON format.
pub fn export_standard_file(
    document: &DtsDocument,
    path: impl AsRef<Path>,
) -> Result<(), AdapterError> {
    let json = export_standard_str(document)?;
    fs::write(path, json)?;
    Ok(())
}

/// Export a document to the standard JSON format while extracting template metadata.
pub fn export_standard_str_with_template(
    rendered_source: &str,
    template_source: &str,
) -> Result<String, AdapterError> {
    export_standard_str_with_template_mode(
        rendered_source,
        template_source,
        TemplateParseMode::StripPlaceholders,
    )
}

/// Export a document to JSON with template metadata using the requested parse mode.
pub fn export_standard_str_with_template_mode(
    rendered_source: &str,
    template_source: &str,
    mode: TemplateParseMode,
) -> Result<String, AdapterError> {
    let TemplateCapture { values, fragments } =
        capture_template_sections(template_source, rendered_source)?;
    let source_to_parse = match mode {
        TemplateParseMode::StripPlaceholders => {
            strip_template_fragments(rendered_source, &fragments)
        }
        TemplateParseMode::FullDocument => rendered_source.to_string(),
    };
    let document = DtsDocument::parse_str(&source_to_parse).map_err(DtsError::from)?;
    let mut layout = AdapterLayout::from_document(&document);
    apply_captured_template_values(&mut layout, values);
    Ok(layout.to_standard_json()?)
}

/// Export a document to the standard JSON format using regex delimiters instead of a template.
pub fn export_standard_str_with_regex_extractions(
    rendered_source: &str,
    extractors: &[RegexExtractionConfig],
) -> Result<String, AdapterError> {
    let TemplateCapture { values, fragments } = capture_regex_sections(rendered_source, extractors);
    let strip_keys: HashSet<&str> = extractors
        .iter()
        .filter(|cfg| cfg.strip_for_parse)
        .map(|cfg| cfg.placeholder.as_str())
        .collect();
    let source_to_parse =
        strip_fragments_matching(rendered_source, &fragments, |key| strip_keys.contains(key));
    let document = DtsDocument::parse_str(&source_to_parse).map_err(DtsError::from)?;
    let mut layout = AdapterLayout::from_document(&document);
    apply_captured_template_values(&mut layout, values);
    Ok(layout.to_standard_json()?)
}

/// Export a DTS file to the standard JSON format using a template for metadata extraction.
pub fn export_standard_file_with_template(
    dts_path: impl AsRef<Path>,
    template_path: impl AsRef<Path>,
    json_path: impl AsRef<Path>,
) -> Result<(), AdapterError> {
    let rendered = fs::read_to_string(&dts_path)?;
    let template = fs::read_to_string(template_path)?;
    let json = export_standard_str_with_template(&rendered, &template)?;
    fs::write(json_path, json)?;
    Ok(())
}

/// Apply a standard JSON payload to a document template.
pub fn import_standard_str(
    json: &str,
    base_document: DtsDocument,
) -> Result<DtsDocument, AdapterError> {
    let layout = AdapterLayout::from_standard_json(json)?;
    Ok(layout.apply_to_document(base_document)?)
}

/// Read a standard JSON file and apply it to the provided document.
pub fn import_standard_file(
    path: impl AsRef<Path>,
    base_document: DtsDocument,
) -> Result<DtsDocument, AdapterError> {
    let text = fs::read_to_string(path)?;
    import_standard_str(&text, base_document)
}

/// Apply a standard JSON payload to a DTS template provided as a string.
pub fn import_standard_str_with_template(
    json: &str,
    template_source: &str,
) -> Result<DtsDocument, AdapterError> {
    let layout = AdapterLayout::from_standard_json(json)?;
    if !template_contains_placeholders(template_source) {
        let template = DtsDocument::parse_str(template_source).map_err(DtsError::from)?;
        return Ok(layout.apply_to_document(template)?);
    }

    let rendered = render_layout_with_template(&layout, template_source)?;
    let document = DtsDocument::parse_str(&rendered).map_err(DtsError::from)?;
    Ok(document)
}

/// Read the JSON and template files, generating a new DTS document from both.
pub fn import_standard_file_with_template(
    json_path: impl AsRef<Path>,
    template_path: impl AsRef<Path>,
) -> Result<DtsDocument, AdapterError> {
    let json = fs::read_to_string(json_path)?;
    let template = fs::read_to_string(template_path)?;
    import_standard_str_with_template(&json, &template)
}

/// Import a standard JSON layout using the template referenced by the keyboard profile.
pub fn import_standard_file_for_profile(
    json_path: impl AsRef<Path>,
    profile: &KeyboardProfileDoc,
    profile_root: impl AsRef<Path>,
) -> Result<DtsDocument, AdapterError> {
    let template_path = resolve_profile_template_path(&profile.layout.template, profile_root);
    import_standard_file_with_template(json_path, template_path)
}

/// Render a template-based DTS string directly from the standard JSON layout.
/// The returned string preserves the template's whitespace instead of going through serialization.
pub fn render_standard_template(json: &str, template_source: &str) -> Result<String, AdapterError> {
    let layout = AdapterLayout::from_standard_json(json)?;
    let rendered = render_layout_with_template(&layout, template_source)?;
    DtsDocument::parse_str(&rendered).map_err(DtsError::from)?;
    Ok(rendered)
}

/// Render the profile's template directly from the JSON payload.
pub fn render_standard_template_for_profile(
    json: &str,
    profile: &KeyboardProfileDoc,
    profile_root: impl AsRef<Path>,
) -> Result<String, AdapterError> {
    let template_path = resolve_profile_template_path(&profile.layout.template, profile_root);
    let source = fs::read_to_string(&template_path)?;
    render_standard_template(json, &source)
}

fn resolve_profile_template_path(template: &str, profile_root: impl AsRef<Path>) -> PathBuf {
    let path = PathBuf::from(template);
    if path.is_absolute() {
        path
    } else {
        profile_root.as_ref().join(path)
    }
}
