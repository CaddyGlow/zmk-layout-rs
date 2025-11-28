#![cfg(feature = "ancpp-preprocessor")]

use std::{
    collections::HashMap,
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::cli::{app::PreprocessorArgs, error::CliError};
use zmk_layout_core::preprocessor::PreprocessorConfig;

pub fn build_config(
    args: &PreprocessorArgs,
    layout_path: &Path,
) -> Result<PreprocessorConfig, CliError> {
    let mut user_dirs: HashSet<PathBuf> = args.include.iter().cloned().collect();
    if let Some(parent) = layout_path.parent() {
        user_dirs.insert(parent.to_path_buf());
    }

    let system_dirs: HashSet<PathBuf> = args.system_include.iter().cloned().collect();
    let predefinitions = parse_definitions(&args.define)?;

    Ok(PreprocessorConfig {
        user_include_dirs: user_dirs.into_iter().collect(),
        system_include_dirs: system_dirs.into_iter().collect(),
        predefinitions,
        resolve_relative_paths: !args.no_resolve_relative,
    })
}

fn parse_definitions(defines: &[String]) -> Result<HashMap<String, String>, CliError> {
    let mut predefs = HashMap::new();
    for def in defines {
        let mut parts = def.splitn(2, '=');
        let key = parts
            .next()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .ok_or_else(|| {
                CliError::InvalidArgument(format!(
                    "invalid --cpp-define `{def}`, expected NAME or NAME=VALUE"
                ))
            })?;
        let value = parts.next().unwrap_or("1");
        predefs.insert(key.to_string(), value.to_string());
    }
    Ok(predefs)
}
