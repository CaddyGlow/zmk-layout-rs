#![cfg(feature = "ancpp-preprocessor")]

use std::{collections::HashMap, path::{Path, PathBuf}};

use ancpp::{
    PreprocessFileError,
    FILE_NUMBER_SOURCE_FILE_BEGIN,
    header_file_cache::HeaderFileCache,
    native_file_provider::NativeFileProvider,
    process_source_file,
    prompt::Prompt,
    token::Token,
    TokenWithLocation,
};
use thiserror::Error;

use crate::{dts::DtsDocument, tokenizer::LayoutError};
use regex::Regex;
use tempfile::Builder;

/// Result of running the `ancpp` preprocessor over a DTS/DTSI file.
pub struct AncppOutput {
    pub expanded: String,
    pub prompts: Vec<Prompt>,
}

/// Configuration for running `ancpp`.
#[derive(Debug, Clone)]
pub struct PreprocessorConfig {
    pub user_include_dirs: Vec<PathBuf>,
    pub system_include_dirs: Vec<PathBuf>,
    pub predefinitions: HashMap<String, String>,
    pub resolve_relative_paths: bool,
}

impl Default for PreprocessorConfig {
    fn default() -> Self {
        Self {
            user_include_dirs: Vec::new(),
            system_include_dirs: Vec::new(),
            predefinitions: HashMap::new(),
            resolve_relative_paths: true,
        }
    }
}

/// Errors returned while preprocessing/parsing with `ancpp`.
#[derive(Debug, Error)]
pub enum AncppError {
    #[error("preprocessor failed: {0:?}")]
    Preprocessor(PreprocessFileError),
    #[error(transparent)]
    Parse(#[from] LayoutError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<PreprocessFileError> for AncppError {
    fn from(err: PreprocessFileError) -> Self {
        Self::Preprocessor(err)
    }
}

/// Preprocess a DTS/DTSI file with `ancpp`, returning normalized text suitable for the existing parser.
///
/// `user_include_dirs` should contain paths where quoted includes (e.g. "helper.h") live, such as the
/// zmk-helpers directory. `system_include_dirs` can point at Zephyr/ZMK headers that are usually pulled
/// in with angle brackets. `predefinitions` lets callers seed values for things like `HOST_OS`.
pub fn preprocess_file_to_string(
    source_path: &Path,
    user_include_dirs: &[PathBuf],
    system_include_dirs: &[PathBuf],
    predefinitions: &HashMap<String, String>,
    resolve_relative_paths: bool,
) -> Result<AncppOutput, AncppError> {
    let canonical_path = source_path.canonicalize()?;
    let relative_path = source_path
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("source.dts"));

    let provider = NativeFileProvider::new(user_include_dirs, system_include_dirs);
    let mut cache = HeaderFileCache::new();

    // DTS doesn’t reserve any keywords in the C sense, so keep this empty.
    let reserved: [&str; 0] = [];

    let result = process_source_file(
        &provider,
        &mut cache,
        &reserved,
        predefinitions,
        resolve_relative_paths,
        FILE_NUMBER_SOURCE_FILE_BEGIN + 1,
        &relative_path,
        &canonical_path,
    )?;

    let expanded = tokens_to_string(&result.output);

    Ok(AncppOutput {
        expanded,
        prompts: result.prompts,
    })
}

/// Preprocess a DTS/DTSI file using a configuration struct.
pub fn preprocess_layout(
    source_path: &Path,
    config: &PreprocessorConfig,
) -> Result<AncppOutput, AncppError> {
    let source_text = std::fs::read_to_string(source_path)?;
    let sanitized = sanitize_non_directive_hashes(&source_text);
    let sanitized = replace_has_include(&sanitized);

    let parent = source_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let tmp = Builder::new()
        .prefix(".zmk-ancpp-")
        .suffix(".dts")
        .tempfile_in(&parent)?;
    std::fs::write(tmp.path(), sanitized.as_bytes())?;

    let preprocessed = preprocess_file_to_string(
        tmp.path(),
        &config.user_include_dirs,
        &config.system_include_dirs,
        &config.predefinitions,
        config.resolve_relative_paths,
    )
    .or_else(|err| match err {
        AncppError::Preprocessor(_) => {
            eprintln!(
                "warning: ancpp preprocessing failed for {}; falling back to raw file",
                source_path.display()
            );
            Ok(AncppOutput {
                expanded: source_text.clone(),
                prompts: Vec::new(),
            })
        }
        other => Err(other),
    })?;

    let restored = restore_non_directive_hashes(&preprocessed.expanded);

    Ok(AncppOutput {
        expanded: restored,
        prompts: preprocessed.prompts,
    })
}

/// Convenience helper: preprocess then parse into a `DtsDocument`.
pub fn parse_file_with_ancpp(
    source_path: &Path,
    user_include_dirs: &[PathBuf],
    system_include_dirs: &[PathBuf],
    predefinitions: &HashMap<String, String>,
    resolve_relative_paths: bool,
) -> Result<DtsDocument, AncppError> {
    let config = PreprocessorConfig {
        user_include_dirs: user_include_dirs.to_vec(),
        system_include_dirs: system_include_dirs.to_vec(),
        predefinitions: predefinitions.clone(),
        resolve_relative_paths,
    };
    let preprocessed = preprocess_layout(source_path, &config)?;

    let doc = DtsDocument::parse_str(&preprocessed.expanded)?;
    Ok(doc)
}

fn tokens_to_string(tokens: &[TokenWithLocation]) -> String {
    tokens
        .iter()
        .map(|TokenWithLocation { token, .. }| token_to_text(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn token_to_text(token: &Token) -> String {
    token.to_string()
}

fn sanitize_non_directive_hashes(text: &str) -> String {
    // Replace leading `#foo-bar` that are not preprocessor directives with placeholder tokens
    // so ancpp doesn't treat them as invalid directives. Hyphens also get escaped so the
    // placeholder remains a valid identifier for ancpp's lexer.
    // This is a conservative line-based transform: if the first non-space character is `#`
    // and the token is not a known directive keyword, rewrite that leading `#` segment.
    let directive_re = Regex::new(r"^(include|define|if|ifdef|ifndef|elif|else|endif|undef|pragma|error|warning|embed)(\s|$)").expect("regex");
    let mut output = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let rest = trimmed[1..].trim_start();
            if !directive_re.is_match(rest) {
                // Rewrite the first `#<token>` occurrence on the line.
                let replaced = line.replacen(
                    '#',
                    "__ZMK_HASH__",
                    1,
                );
                let replaced = replaced.replace("-", "__ZMK_DASH__");
                output.push_str(&replaced);
                output.push('\n');
                continue;
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn restore_non_directive_hashes(text: &str) -> String {
    text
        .replace("__ZMK_DASH__", "-")
        .replace("__ZMK_HASH__", "#")
}

fn replace_has_include(text: &str) -> String {
    let call_re = Regex::new(r"__has_include(?:_next)?\s*\([^\)]*\)").expect("regex");
    let ident_re = Regex::new(r"\b__has_include(_next)?\b").expect("regex");
    let pass_one = call_re.replace_all(text, "1");
    ident_re.replace_all(&pass_one, "1").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn preprocesses_macros_with_varargs_and_token_pasting() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempdir()?;
        let include_dir = tmp.path().join("include");
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&include_dir)?;
        std::fs::create_dir_all(&src_dir)?;

        let helper_h = r#"
#define CONCAT(a, b) a##b
#define JOIN(a, b) CONCAT(a, b)
#define TAKE_TWO(x, y, ...) x y
"#;
        std::fs::write(include_dir.join("helper.h"), helper_h)?;

        let source = r#"
#include "helper.h"
JOIN(foo, bar)
TAKE_TWO(1, 2, 3, 4)
"#;
        let source_path = src_dir.join("keymap.dts");
        std::fs::write(&source_path, source)?;

        let preprocessed = preprocess_file_to_string(
            &source_path,
            &[include_dir.clone()],
            &[],
            &HashMap::new(),
            true,
        )?;

        let text = preprocessed.expanded;
        assert!(text.contains("foobar"), "token pasting failed: {text}");
        assert!(text.contains("1 2"), "varargs trimming failed: {text}");

        Ok(())
    }
}
