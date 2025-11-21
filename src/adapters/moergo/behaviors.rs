use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use serde::Deserialize;

use crate::adapters::bundle::BundleError;

const OVERRIDE_PATH: &str = "profiles/vendors/moergo/behaviors.json";
const EMBEDDED_BEHAVIORS_JSON: &str =
    include_str!("../../../profiles/vendors/moergo/behaviors.json");

#[derive(Debug, Clone, Default)]
pub struct BehaviorMetadata {
    includes_by_code: BTreeMap<String, Vec<String>>,
    required_configs_by_code: BTreeMap<String, Vec<String>>,
}

impl BehaviorMetadata {
    pub fn includes_for(&self, code: &str) -> Option<&[String]> {
        self.includes_by_code.get(code).map(|v| v.as_slice())
    }

    pub fn required_configs_for(&self, code: &str) -> Option<&[String]> {
        self.required_configs_by_code
            .get(code)
            .map(|v| v.as_slice())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct BehaviorRecord {
    code: String,
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_requires_config")]
    requires_config: Vec<String>,
}

/// Load behavior metadata, preferring an on-disk override when present.
pub fn load_behavior_metadata() -> Result<BehaviorMetadata, BundleError> {
    let source = load_overridable()?;
    let records: Vec<BehaviorRecord> = serde_json::from_str(&source)?;
    let mut includes_by_code: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut required_configs_by_code: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for record in records {
        if record.code.trim().is_empty() {
            continue;
        }
        let code = record.code.trim().to_string();
        merge_vec_map(&mut includes_by_code, &code, record.includes);
        merge_vec_map(&mut required_configs_by_code, &code, record.requires_config);
    }

    Ok(BehaviorMetadata {
        includes_by_code,
        required_configs_by_code,
    })
}

fn load_overridable() -> Result<String, BundleError> {
    let path = Path::new(OVERRIDE_PATH);
    if path.exists() {
        Ok(fs::read_to_string(path)?)
    } else {
        Ok(EMBEDDED_BEHAVIORS_JSON.to_string())
    }
}

fn merge_vec_map(target: &mut BTreeMap<String, Vec<String>>, key: &str, values: Vec<String>) {
    let mut merged: BTreeSet<String> = target.remove(key).unwrap_or_default().into_iter().collect();
    merged.extend(values.into_iter().map(|v| v.trim().to_string()));
    merged.retain(|s| !s.is_empty());
    target.insert(key.to_string(), merged.into_iter().collect());
}

fn deserialize_requires_config<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ConfigList {
        One(String),
        Many(Vec<String>),
    }

    let parsed = ConfigList::deserialize(deserializer)?;
    Ok(match parsed {
        ConfigList::One(value) => vec![value],
        ConfigList::Many(values) => values,
    })
}
