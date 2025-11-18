use std::{fs, path::Path};

use crate::dts::{DtsDocument, DtsError};

use super::{
    AdapterError,
    layout::AdapterLayout,
    render::render_layout_with_template,
    template::{merge_template_metadata, template_contains_placeholders},
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
    document: &DtsDocument,
    rendered_source: &str,
    template_source: &str,
) -> Result<String, AdapterError> {
    let mut layout = AdapterLayout::from_document(document);
    merge_template_metadata(&mut layout, template_source, rendered_source)?;
    Ok(layout.to_standard_json()?)
}

/// Export a DTS file to the standard JSON format using a template for metadata extraction.
pub fn export_standard_file_with_template(
    dts_path: impl AsRef<Path>,
    template_path: impl AsRef<Path>,
    json_path: impl AsRef<Path>,
) -> Result<(), AdapterError> {
    let rendered = fs::read_to_string(&dts_path)?;
    let document = DtsDocument::parse_str(&rendered).map_err(DtsError::from)?;
    let template = fs::read_to_string(template_path)?;
    let json = export_standard_str_with_template(&document, &rendered, &template)?;
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

    let rendered = render_layout_with_template(&layout, template_source);
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

/// Render a template-based DTS string directly from the standard JSON layout.
/// The returned string preserves the template's whitespace instead of going through serialization.
pub fn render_standard_template(json: &str, template_source: &str) -> Result<String, AdapterError> {
    let layout = AdapterLayout::from_standard_json(json)?;
    let rendered = render_layout_with_template(&layout, template_source);
    DtsDocument::parse_str(&rendered).map_err(DtsError::from)?;
    Ok(rendered)
}
