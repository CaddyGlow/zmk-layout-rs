use std::{collections::BTreeMap, fs::OpenOptions, io::Write, path::Path};

use crate::build::error::BuildError;

/// Append Kconfig definitions to a file, creating it when necessary.
pub fn append_kconfig_defs(path: &Path, defs: &BTreeMap<String, String>) -> Result<(), BuildError> {
    if defs.is_empty() {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(BuildError::Io)?;
    for (key, value) in defs {
        writeln!(file, "{key}={value}").map_err(BuildError::Io)?;
    }
    Ok(())
}
