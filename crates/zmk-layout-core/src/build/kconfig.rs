use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use serde_json::Value as JsonValue;
use thiserror::Error;
use toml::Value as TomlValue;

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

/// Kconfig parameter type specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KconfigType {
    Bool,
    Int,
    String,
}

impl KconfigType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "bool" => Some(Self::Bool),
            "int" => Some(Self::Int),
            "string" => Some(Self::String),
            _ => None,
        }
    }
}

/// Kconfig option metadata from profile.
#[derive(Debug, Clone)]
pub struct KconfigOption {
    pub name: String,
    pub param_type: KconfigType,
    pub default: Option<String>,
    pub description: Option<String>,
    pub allowed_values: Option<Vec<String>>,
}

/// Kconfig entry with origin tracking for debugging/dry-run.
#[derive(Debug, Clone)]
pub struct KconfigEntry {
    pub key: String,
    pub value: String,
    pub origin: String,
}

/// Source of kconfig configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KconfigSource {
    /// User provided explicit kconfig file path.
    UserFile(PathBuf),
    /// Generate from profile defaults + JSON params + defs.
    Generated,
    /// No kconfig (only defs will be used).
    DefsOnly,
}

/// Result of kconfig resolution.
#[derive(Debug)]
pub struct KconfigResolution {
    /// Path to the generated or staged config.conf file.
    pub config_path: Option<PathBuf>,
    /// All entries with origin tracking (for dry-run display).
    pub entries: Vec<KconfigEntry>,
    /// Warnings generated during resolution.
    pub warnings: Vec<String>,
}

/// Kconfig resolver for firmware builds.
pub struct KconfigResolver {
    source: KconfigSource,
    kconfig_defs: BTreeMap<String, String>,
}

impl KconfigResolver {
    /// Create a resolver that generates kconfig from profile/JSON.
    pub fn new_generated(kconfig_defs: BTreeMap<String, String>) -> Self {
        Self {
            source: KconfigSource::Generated,
            kconfig_defs,
        }
    }

    /// Create a resolver that uses a user-provided kconfig file.
    pub fn new_from_file(path: PathBuf, kconfig_defs: BTreeMap<String, String>) -> Self {
        Self {
            source: KconfigSource::UserFile(path),
            kconfig_defs,
        }
    }

    /// Create a resolver that only uses defs (no base config).
    pub fn new_defs_only(kconfig_defs: BTreeMap<String, String>) -> Self {
        Self {
            source: KconfigSource::DefsOnly,
            kconfig_defs,
        }
    }

    /// Get the source type.
    pub fn source(&self) -> &KconfigSource {
        &self.source
    }

    /// Resolve kconfig and generate config.conf under workspace layout dir.
    pub fn resolve(
        &self,
        workspace_layout_dir: &Path,
        profile_kconfig_map: Option<&BTreeMap<String, TomlValue>>,
        hardware_defaults: Option<&BTreeMap<String, String>>,
        firmware_kconfig: Option<&BTreeMap<String, String>>,
        json_config_params: Option<&[JsonValue]>,
    ) -> Result<KconfigResolution, KconfigError> {
        let config_path = workspace_layout_dir.join("config.conf");

        match &self.source {
            KconfigSource::UserFile(user_path) => {
                self.resolve_from_user_file(user_path, &config_path)
            }
            KconfigSource::Generated => self.resolve_generated(
                &config_path,
                profile_kconfig_map,
                hardware_defaults,
                firmware_kconfig,
                json_config_params,
            ),
            KconfigSource::DefsOnly => self.resolve_defs_only(&config_path),
        }
    }

    fn resolve_defs_only(&self, config_path: &Path) -> Result<KconfigResolution, KconfigError> {
        if self.kconfig_defs.is_empty() {
            return Ok(KconfigResolution {
                config_path: None,
                entries: vec![],
                warnings: vec![],
            });
        }

        let entries: Vec<KconfigEntry> = self
            .kconfig_defs
            .iter()
            .map(|(key, value)| KconfigEntry {
                key: key.clone(),
                value: value.clone(),
                origin: "kconfig_defs".to_string(),
            })
            .collect();

        self.write_config_file(config_path, &entries)?;

        Ok(KconfigResolution {
            config_path: Some(config_path.to_path_buf()),
            entries,
            warnings: vec![],
        })
    }

    fn resolve_from_user_file(
        &self,
        user_path: &Path,
        dest_path: &Path,
    ) -> Result<KconfigResolution, KconfigError> {
        // Stage/copy user-provided kconfig into workspace
        fs::copy(user_path, dest_path).map_err(|source| KconfigError::FileOperation {
            path: user_path.to_path_buf(),
            source,
        })?;

        // Parse existing entries for tracking
        let content =
            fs::read_to_string(dest_path).map_err(|source| KconfigError::FileOperation {
                path: dest_path.to_path_buf(),
                source,
            })?;
        let mut entries = parse_kconfig_entries(&content, "user-provided");

        // Append kconfig_defs (treating -D/--kconfig-def as append)
        append_kconfig_defs(dest_path, &self.kconfig_defs)?;

        // Add defs to entries for tracking
        for (key, value) in &self.kconfig_defs {
            entries.push(KconfigEntry {
                key: key.clone(),
                value: value.clone(),
                origin: "kconfig_defs".to_string(),
            });
        }

        Ok(KconfigResolution {
            config_path: Some(dest_path.to_path_buf()),
            entries,
            warnings: vec![],
        })
    }

    fn resolve_generated(
        &self,
        config_path: &Path,
        profile_kconfig_map: Option<&BTreeMap<String, TomlValue>>,
        hardware_defaults: Option<&BTreeMap<String, String>>,
        firmware_kconfig: Option<&BTreeMap<String, String>>,
        json_config_params: Option<&[JsonValue]>,
    ) -> Result<KconfigResolution, KconfigError> {
        let mut entries = Vec::new();
        let mut merged: BTreeMap<String, String> = BTreeMap::new();
        let mut warnings = Vec::new();

        // Layer 1: Hardware defaults
        if let Some(defaults) = hardware_defaults {
            for (key, value) in defaults {
                check_conflict(&merged, key, value, "hardware defaults")?;
                merged.insert(key.clone(), value.clone());
                entries.push(KconfigEntry {
                    key: key.clone(),
                    value: value.clone(),
                    origin: "hardware defaults".to_string(),
                });
            }
        }

        // Layer 2: Firmware version kconfig
        if let Some(fw_kconfig) = firmware_kconfig {
            for (key, value) in fw_kconfig {
                check_conflict(&merged, key, value, "firmware version")?;
                merged.insert(key.clone(), value.clone());
                entries.push(KconfigEntry {
                    key: key.clone(),
                    value: value.clone(),
                    origin: "firmware version".to_string(),
                });
            }
        }

        // Layer 3: Mapped params from JSON config
        if let (Some(kconfig_map), Some(params)) = (profile_kconfig_map, json_config_params) {
            let (mapped, map_warnings) = self.map_json_params_to_kconfig(kconfig_map, params)?;
            warnings.extend(map_warnings);
            for (key, value, param_name) in mapped {
                let origin = format!("JSON param: {}", param_name);
                check_conflict(&merged, &key, &value, &origin)?;
                merged.insert(key.clone(), value.clone());
                entries.push(KconfigEntry {
                    key,
                    value,
                    origin,
                });
            }
        }

        // Layer 4: kconfig_defs (may override prior values)
        for (key, value) in &self.kconfig_defs {
            // kconfig_defs can override prior values, so no conflict check
            merged.insert(key.clone(), value.clone());
            entries.push(KconfigEntry {
                key: key.clone(),
                value: value.clone(),
                origin: "kconfig_defs".to_string(),
            });
        }

        if entries.is_empty() {
            return Ok(KconfigResolution {
                config_path: None,
                entries,
                warnings,
            });
        }

        // Write config.conf
        self.write_config_file(config_path, &entries)?;

        Ok(KconfigResolution {
            config_path: Some(config_path.to_path_buf()),
            entries,
            warnings,
        })
    }

    fn map_json_params_to_kconfig(
        &self,
        kconfig_map: &BTreeMap<String, TomlValue>,
        params: &[JsonValue],
    ) -> Result<(Vec<(String, String, String)>, Vec<String>), KconfigError> {
        let mut results = Vec::new();
        let mut warnings = Vec::new();

        // Parse kconfig_options from profile
        let options = parse_kconfig_options(kconfig_map)?;

        // Process each JSON param
        for param in params {
            let param_obj = param.as_object().ok_or_else(|| {
                KconfigError::InvalidFormat("config_parameter must be object".to_string())
            })?;

            let param_name = param_obj
                .get("paramName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    KconfigError::InvalidFormat("config_parameter missing paramName".to_string())
                })?;

            let value_json = param_obj.get("value").ok_or_else(|| {
                KconfigError::InvalidFormat(format!(
                    "config_parameter '{}' missing value",
                    param_name
                ))
            })?;

            // Look up param in kconfig_options
            if let Some(option) = options.get(param_name) {
                let value_str = self.convert_and_validate_value(value_json, option, param_name)?;
                results.push((option.name.clone(), value_str, param_name.to_string()));
            } else {
                // Warn and skip if param not found in map
                warnings.push(format!(
                    "config_parameter '{}' not found in profile kconfig_options, skipping",
                    param_name
                ));
            }
        }

        // Apply defaults for params missing in JSON but present in map with defaults
        for (param_name, option) in &options {
            let param_exists = params.iter().any(|p| {
                p.as_object()
                    .and_then(|o| o.get("paramName"))
                    .and_then(|v| v.as_str())
                    == Some(param_name)
            });

            if !param_exists {
                if let Some(default_val) = &option.default {
                    results.push((
                        option.name.clone(),
                        default_val.clone(),
                        format!("{} (default)", param_name),
                    ));
                }
            }
        }

        Ok((results, warnings))
    }

    fn convert_and_validate_value(
        &self,
        value: &JsonValue,
        option: &KconfigOption,
        param_name: &str,
    ) -> Result<String, KconfigError> {
        let value_str = match value {
            JsonValue::String(s) => s.clone(),
            JsonValue::Bool(b) => b.to_string(),
            JsonValue::Number(n) => n.to_string(),
            _ => {
                return Err(KconfigError::InvalidValue {
                    param: param_name.to_string(),
                    reason: "value must be string, bool, or number".to_string(),
                })
            }
        };

        // Type conversion
        let converted = match option.param_type {
            KconfigType::Bool => convert_to_bool(&value_str, param_name)?,
            KconfigType::Int => convert_to_int(&value_str, param_name)?,
            KconfigType::String => value_str.clone(),
        };

        // Validate against allowed values
        if let Some(allowed) = &option.allowed_values {
            if !allowed.contains(&converted) {
                return Err(KconfigError::InvalidValue {
                    param: param_name.to_string(),
                    reason: format!(
                        "value '{}' not in allowed values: {:?}",
                        converted, allowed
                    ),
                });
            }
        }

        Ok(converted)
    }

    fn write_config_file(&self, path: &Path, entries: &[KconfigEntry]) -> Result<(), KconfigError> {
        let mut content = String::new();
        content.push_str("# Generated Kconfig configuration\n");
        content.push_str("# This file was automatically generated\n\n");

        let mut current_origin = String::new();
        for entry in entries {
            if entry.origin != current_origin {
                content.push_str(&format!("\n# Source: {}\n", entry.origin));
                current_origin.clone_from(&entry.origin);
            }
            content.push_str(&format!("{}={}\n", entry.key, entry.value));
        }

        fs::write(path, content).map_err(|source| KconfigError::FileOperation {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(())
    }

    /// Format entries for dry-run display.
    pub fn format_dry_run(&self, resolution: &KconfigResolution) -> String {
        let mut output = String::new();

        if resolution.entries.is_empty() {
            output.push_str("  (no kconfig entries)\n");
            return output;
        }

        let mut current_origin = String::new();
        for entry in &resolution.entries {
            if entry.origin != current_origin {
                output.push_str(&format!("  # {}\n", entry.origin));
                current_origin.clone_from(&entry.origin);
            }
            output.push_str(&format!("  {}={}\n", entry.key, entry.value));
        }

        if !resolution.warnings.is_empty() {
            output.push_str("\n  Warnings:\n");
            for warning in &resolution.warnings {
                output.push_str(&format!("    - {}\n", warning));
            }
        }

        output
    }
}

fn parse_kconfig_entries(content: &str, origin: &str) -> Vec<KconfigEntry> {
    let mut entries = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            entries.push(KconfigEntry {
                key: key.trim().to_string(),
                value: value.trim().to_string(),
                origin: origin.to_string(),
            });
        }
    }
    entries
}

fn parse_kconfig_options(
    kconfig_map: &BTreeMap<String, TomlValue>,
) -> Result<BTreeMap<String, KconfigOption>, KconfigError> {
    let mut options = BTreeMap::new();

    for (param_name, value) in kconfig_map {
        let table = value.as_table().ok_or_else(|| {
            KconfigError::InvalidFormat(format!(
                "kconfig_options.{} must be a table",
                param_name
            ))
        })?;

        let name = table
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                KconfigError::InvalidFormat(format!(
                    "kconfig_options.{}.name is required",
                    param_name
                ))
            })?
            .to_string();

        let type_str = table
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                KconfigError::InvalidFormat(format!(
                    "kconfig_options.{}.type is required",
                    param_name
                ))
            })?;

        let param_type = KconfigType::from_str(type_str).ok_or_else(|| {
            KconfigError::InvalidFormat(format!(
                "kconfig_options.{}.type '{}' is invalid (must be bool, int, or string)",
                param_name, type_str
            ))
        })?;

        let default = match table.get("default") {
            Some(TomlValue::String(s)) => Some(s.clone()),
            Some(TomlValue::Integer(i)) => Some(i.to_string()),
            Some(TomlValue::Boolean(b)) => Some(if *b {
                "y".to_string()
            } else {
                "n".to_string()
            }),
            Some(_) => {
                return Err(KconfigError::InvalidFormat(format!(
                    "kconfig_options.{}.default has invalid type",
                    param_name
                )))
            }
            None => None,
        };

        let description = table
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let allowed_values = table.get("allowed_values").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|val| val.as_str().map(|s| s.to_string()))
                    .collect()
            })
        });

        options.insert(
            param_name.clone(),
            KconfigOption {
                name,
                param_type,
                default,
                description,
                allowed_values,
            },
        );
    }

    Ok(options)
}

fn check_conflict(
    merged: &BTreeMap<String, String>,
    key: &str,
    value: &str,
    origin: &str,
) -> Result<(), KconfigError> {
    if let Some(existing) = merged.get(key) {
        if existing != value {
            return Err(KconfigError::Conflict {
                key: key.to_string(),
                existing: existing.clone(),
                new: value.to_string(),
                origin: origin.to_string(),
            });
        }
    }
    Ok(())
}

fn convert_to_bool(value: &str, param_name: &str) -> Result<String, KconfigError> {
    match value.to_lowercase().as_str() {
        "y" | "true" | "1" => Ok("y".to_string()),
        "n" | "false" | "0" => Ok("n".to_string()),
        _ => Err(KconfigError::InvalidValue {
            param: param_name.to_string(),
            reason: format!(
                "cannot convert '{}' to bool (expected y/n/true/false)",
                value
            ),
        }),
    }
}

fn convert_to_int(value: &str, param_name: &str) -> Result<String, KconfigError> {
    value
        .parse::<i64>()
        .map(|i| i.to_string())
        .map_err(|_| KconfigError::InvalidValue {
            param: param_name.to_string(),
            reason: format!("cannot convert '{}' to int", value),
        })
}

/// Errors during kconfig resolution.
#[derive(Debug, Error)]
pub enum KconfigError {
    #[error("invalid kconfig format: {0}")]
    InvalidFormat(String),

    #[error("invalid value for parameter '{param}': {reason}")]
    InvalidValue { param: String, reason: String },

    #[error("kconfig conflict for '{key}': existing value '{existing}' conflicts with new value '{new}' from {origin}")]
    Conflict {
        key: String,
        existing: String,
        new: String,
        origin: String,
    },

    #[error("file operation failed for {path}: {source}")]
    FileOperation {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error(transparent)]
    BuildError(#[from] BuildError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bool_values() {
        assert_eq!(convert_to_bool("y", "test").unwrap(), "y");
        assert_eq!(convert_to_bool("n", "test").unwrap(), "n");
        assert_eq!(convert_to_bool("true", "test").unwrap(), "y");
        assert_eq!(convert_to_bool("false", "test").unwrap(), "n");
        assert_eq!(convert_to_bool("True", "test").unwrap(), "y");
        assert_eq!(convert_to_bool("FALSE", "test").unwrap(), "n");
        assert_eq!(convert_to_bool("1", "test").unwrap(), "y");
        assert_eq!(convert_to_bool("0", "test").unwrap(), "n");
        assert!(convert_to_bool("maybe", "test").is_err());
    }

    #[test]
    fn converts_int_values() {
        assert_eq!(convert_to_int("42", "test").unwrap(), "42");
        assert_eq!(convert_to_int("-10", "test").unwrap(), "-10");
        assert_eq!(convert_to_int("0", "test").unwrap(), "0");
        assert!(convert_to_int("abc", "test").is_err());
        assert!(convert_to_int("12.5", "test").is_err());
    }

    #[test]
    fn parses_kconfig_entries() {
        let content = "# Comment\nCONFIG_FOO=y\nCONFIG_BAR=123\n\nCONFIG_BAZ=hello";
        let entries = parse_kconfig_entries(content, "test");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key, "CONFIG_FOO");
        assert_eq!(entries[0].value, "y");
        assert_eq!(entries[1].key, "CONFIG_BAR");
        assert_eq!(entries[1].value, "123");
        assert_eq!(entries[2].key, "CONFIG_BAZ");
        assert_eq!(entries[2].value, "hello");
    }

    #[test]
    fn detects_conflicts() {
        let mut merged = BTreeMap::new();
        merged.insert("CONFIG_FOO".to_string(), "y".to_string());

        // Same value is fine
        assert!(check_conflict(&merged, "CONFIG_FOO", "y", "test").is_ok());

        // Different value is error
        assert!(check_conflict(&merged, "CONFIG_FOO", "n", "test").is_err());
    }

    #[test]
    fn resolve_generated_layers_hardware_then_defs() {
        let dir = tempfile::tempdir().unwrap();
        let mut defs = BTreeMap::new();
        defs.insert("CONFIG_OVERRIDE".to_string(), "42".to_string());
        let resolver = KconfigResolver::new_generated(defs);

        let mut hw_defaults = BTreeMap::new();
        hw_defaults.insert("CONFIG_HW".to_string(), "y".to_string());

        let result = resolver
            .resolve(dir.path(), None, Some(&hw_defaults), None, None)
            .unwrap();
        assert!(result.config_path.is_some());
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.entries[0].key, "CONFIG_HW");
        assert_eq!(result.entries[0].origin, "hardware defaults");
        assert_eq!(result.entries[1].key, "CONFIG_OVERRIDE");
        assert_eq!(result.entries[1].origin, "kconfig_defs");

        // Verify the file was written
        let content = fs::read_to_string(result.config_path.unwrap()).unwrap();
        assert!(content.contains("CONFIG_HW=y"));
        assert!(content.contains("CONFIG_OVERRIDE=42"));
    }

    #[test]
    fn resolve_from_user_file_copies_and_appends_defs() {
        let dir = tempfile::tempdir().unwrap();
        let user_file = dir.path().join("user.conf");
        fs::write(&user_file, "CONFIG_BASE=100\n").unwrap();

        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();

        let mut defs = BTreeMap::new();
        defs.insert("CONFIG_EXTRA".to_string(), "n".to_string());
        let resolver = KconfigResolver::new_from_file(user_file, defs);

        let result = resolver
            .resolve(&workspace, None, None, None, None)
            .unwrap();
        assert!(result.config_path.is_some());
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.entries[0].key, "CONFIG_BASE");
        assert_eq!(result.entries[0].origin, "user-provided");
        assert_eq!(result.entries[1].key, "CONFIG_EXTRA");
        assert_eq!(result.entries[1].origin, "kconfig_defs");

        let content = fs::read_to_string(result.config_path.unwrap()).unwrap();
        assert!(content.contains("CONFIG_BASE=100"));
        assert!(content.contains("CONFIG_EXTRA=n"));
    }

    #[test]
    fn resolve_defs_only_skips_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = KconfigResolver::new_defs_only(BTreeMap::new());
        let result = resolver
            .resolve(dir.path(), None, None, None, None)
            .unwrap();
        assert!(result.config_path.is_none());
        assert!(result.entries.is_empty());
    }

    #[test]
    fn resolve_generated_maps_json_params() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = KconfigResolver::new_generated(BTreeMap::new());

        // Build a kconfig_options map
        let mut kconfig_map = BTreeMap::new();
        let mut option_table = toml::map::Map::new();
        option_table.insert("name".into(), TomlValue::String("CONFIG_ZMK_SLEEP".into()));
        option_table.insert("type".into(), TomlValue::String("bool".into()));
        option_table.insert("default".into(), TomlValue::Boolean(false));
        kconfig_map.insert("DEEP_SLEEP".to_string(), TomlValue::Table(option_table));

        // Build JSON config_parameters
        let params = vec![serde_json::json!({
            "paramName": "DEEP_SLEEP",
            "value": "true"
        })];

        let result = resolver
            .resolve(dir.path(), Some(&kconfig_map), None, None, Some(&params))
            .unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].key, "CONFIG_ZMK_SLEEP");
        assert_eq!(result.entries[0].value, "y"); // true -> y for bool
    }

    #[test]
    fn resolve_generated_applies_defaults_for_missing_params() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = KconfigResolver::new_generated(BTreeMap::new());

        let mut kconfig_map = BTreeMap::new();
        let mut option_table = toml::map::Map::new();
        option_table.insert(
            "name".into(),
            TomlValue::String("CONFIG_ZMK_BATTERY_REPORT_INTERVAL".into()),
        );
        option_table.insert("type".into(), TomlValue::String("int".into()));
        option_table.insert("default".into(), TomlValue::Integer(600));
        kconfig_map.insert(
            "BATTERY_REPORT_INTERVAL_SEC".to_string(),
            TomlValue::Table(option_table),
        );

        // No JSON params provided -- should get default
        let result = resolver
            .resolve(dir.path(), Some(&kconfig_map), None, None, Some(&[]))
            .unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(
            result.entries[0].key,
            "CONFIG_ZMK_BATTERY_REPORT_INTERVAL"
        );
        assert_eq!(result.entries[0].value, "600");
    }

    #[test]
    fn defs_override_prior_layers() {
        let dir = tempfile::tempdir().unwrap();
        let mut defs = BTreeMap::new();
        defs.insert("CONFIG_HW".to_string(), "n".to_string());
        let resolver = KconfigResolver::new_generated(defs);

        let mut hw_defaults = BTreeMap::new();
        hw_defaults.insert("CONFIG_HW".to_string(), "y".to_string());

        let result = resolver
            .resolve(dir.path(), None, Some(&hw_defaults), None, None)
            .unwrap();
        // Both entries are tracked, but defs override
        assert_eq!(result.entries.len(), 2);
        // The file should have the override last
        let content = fs::read_to_string(result.config_path.unwrap()).unwrap();
        let lines: Vec<&str> = content
            .lines()
            .filter(|l| l.starts_with("CONFIG_HW="))
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1], "CONFIG_HW=n"); // defs override
    }
}
