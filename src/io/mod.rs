use similar::{ChangeTag, TextDiff};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

use crate::{
    adapters::standard::AdapterLayout,
    dts::DtsDocument,
    adapters::AdapterError,
    keymap::KeymapDocument,
    layout_handle::{LayoutHandle, LayoutOrigin},
    serialization::SerializeError,
    tasks::TaskFile,
    tokenizer::LayoutError,
};

#[cfg(feature = "ancpp-preprocessor")]
use crate::preprocessor::{AncppError, PreprocessorConfig, preprocess_layout};

/// Unified layout handle type for IO helpers.
pub type LoadedLayout = LayoutHandle;

/// Loaded task file with the source text preserved.
#[derive(Debug)]
pub struct LoadedTaskFile {
    pub path: PathBuf,
    pub text: String,
    pub file: TaskFile,
}

#[derive(Debug, Error)]
pub enum IoError {
    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse layout {path}: {source}")]
    ParseLayout { path: PathBuf, source: LayoutError },
    #[error("failed to parse task file {path}: {source}")]
    ParseTaskFile {
        path: PathBuf,
        source: crate::tasks::TaskConfigError,
    },
    #[error("failed to serialize layout: {0}")]
    SerializeLayout(#[from] SerializeError),
    #[error("adapter error: {0}")]
    Adapter(#[from] AdapterError),
    #[cfg(feature = "ancpp-preprocessor")]
    #[error("failed to preprocess layout {path}: {source}")]
    PreprocessLayout { path: PathBuf, source: AncppError },
}

/// Read a text file into memory.
pub fn read_text(path: impl AsRef<Path>) -> Result<String, IoError> {
    let path = path.as_ref().to_path_buf();
    fs::read_to_string(&path).map_err(|source| IoError::ReadFile { path, source })
}

/// Write text to a file.
pub fn write_text(path: impl AsRef<Path>, contents: &str) -> Result<(), IoError> {
    let path = path.as_ref().to_path_buf();
    fs::write(&path, contents).map_err(|source| IoError::WriteFile { path, source })
}

/// Load and parse a DTS layout from disk.
pub fn load_layout(path: impl AsRef<Path>) -> Result<LoadedLayout, IoError> {
    let path = path.as_ref().to_path_buf();
    let text = read_text(&path)?;
    let document = DtsDocument::parse_str(&text).map_err(|source| IoError::ParseLayout {
        path: path.clone(),
        source,
    })?;
    let adapter = AdapterLayout::from_document(&document);
    let keymap = KeymapDocument::from(adapter);
    Ok(LayoutHandle {
        source_path: Some(path),
        raw_text: Some(text),
        preprocessed_text: None,
        keymap,
        profile: None,
        origin: LayoutOrigin::DtsFile,
    })
}

/// Load and parse a DTS layout after running the ancpp preprocessor.
#[cfg(feature = "ancpp-preprocessor")]
pub fn load_layout_preprocessed(
    path: impl AsRef<Path>,
    config: &PreprocessorConfig,
) -> Result<LoadedLayout, IoError> {
    let path = path.as_ref().to_path_buf();
    let raw_text = read_text(&path)?;
    let output = preprocess_layout(&path, config).map_err(|source| IoError::PreprocessLayout {
        path: path.clone(),
        source,
    })?;
    let document = DtsDocument::parse_str(&output.expanded).map_err(|source| IoError::ParseLayout {
        path: path.clone(),
        source,
    })?;
    let adapter = AdapterLayout::from_document(&document);
    let keymap = KeymapDocument::from(adapter);
    Ok(LayoutHandle {
        source_path: Some(path),
        raw_text: Some(raw_text),
        preprocessed_text: Some(output.expanded),
        keymap,
        profile: None,
        origin: LayoutOrigin::DtsFile,
    })
}

/// Serialize a keymap back to text.
pub fn serialize_keymap(document: KeymapDocument) -> Result<String, IoError> {
    let adapter: AdapterLayout = document.into();
    let base = minimal_dts_document();
    let updated = adapter.apply_to_document(base).map_err(AdapterError::from)?;
    Ok(updated.to_string()?)
}

/// Serialize a keymap using an existing DTS document as the base.
pub fn serialize_keymap_with_base(
    document: KeymapDocument,
    base_text: &str,
    base_path: impl AsRef<Path>,
) -> Result<String, IoError> {
    let adapter: AdapterLayout = document.into();
    let base = DtsDocument::parse_str(base_text).map_err(|source| IoError::ParseLayout {
        path: base_path.as_ref().to_path_buf(),
        source,
    })?;
    let updated = adapter.apply_to_document(base).map_err(AdapterError::from)?;
    Ok(updated.to_string()?)
}

/// Load and parse a task file from disk.
pub fn load_task_file(path: impl AsRef<Path>) -> Result<LoadedTaskFile, IoError> {
    let path = path.as_ref().to_path_buf();
    let text = read_text(&path)?;
    let file = TaskFile::from_toml_str(&text).map_err(|source| IoError::ParseTaskFile {
        path: path.clone(),
        source,
    })?;
    Ok(LoadedTaskFile { path, text, file })
}

/// Render a unified diff between two documents.
pub fn render_diff(base: &str, updated: &str, base_path: impl AsRef<Path>) -> String {
    let mut output = String::new();
    output.push_str(&format!("--- {}\n", base_path.as_ref().display()));
    output.push_str("+++ updated\n");
    let diff = TextDiff::from_lines(base, updated);
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => '-',
            ChangeTag::Insert => '+',
            ChangeTag::Equal => ' ',
        };
        output.push(sign);
        output.push_str(change.value());
        if !change.value().ends_with('\n') {
            output.push('\n');
        }
    }
    output
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_diff_matches_expected_format() {
        let base = "a\nb\n";
        let updated = "a\nc\n";
        let diff = render_diff(base, updated, "base.dts");
        assert!(
            diff.starts_with("--- base.dts\n+++ updated\n"),
            "unexpected header: {diff}"
        );
        assert!(
            diff.contains("-b\n+c\n"),
            "diff should show delete/insert lines: {diff}"
        );
    }
}
