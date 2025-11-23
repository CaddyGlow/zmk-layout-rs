use similar::{ChangeTag, TextDiff};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

use crate::{
    dts::DtsDocument, providers::KeymapDocument, serialization::SerializeError, tasks::TaskFile,
    tokenizer::LayoutError,
};

#[cfg(feature = "ancpp-preprocessor")]
use crate::preprocessor::{AncppError, PreprocessorConfig, preprocess_layout};

/// Loaded layout with original text and parsed document.
#[derive(Debug)]
pub struct LoadedLayout {
    pub path: PathBuf,
    pub text: String,
    pub document: DtsDocument,
}

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
    Ok(LoadedLayout {
        path,
        text,
        document,
    })
}

/// Load and parse a DTS layout after running the ancpp preprocessor.
#[cfg(feature = "ancpp-preprocessor")]
pub fn load_layout_preprocessed(
    path: impl AsRef<Path>,
    config: &PreprocessorConfig,
) -> Result<LoadedLayout, IoError> {
    let path = path.as_ref().to_path_buf();
    let output = preprocess_layout(&path, config)
        .map_err(|source| IoError::PreprocessLayout { path: path.clone(), source })?;
    let document = DtsDocument::parse_str(&output.expanded).map_err(|source| IoError::ParseLayout {
        path: path.clone(),
        source,
    })?;
    Ok(LoadedLayout {
        path,
        text: output.expanded,
        document,
    })
}

/// Serialize a keymap back to text.
pub fn serialize_keymap(document: KeymapDocument) -> Result<String, IoError> {
    let dts = document.into_document();
    dts.to_string().map_err(IoError::SerializeLayout)
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
