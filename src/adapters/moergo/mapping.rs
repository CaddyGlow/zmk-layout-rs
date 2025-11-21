use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;

use crate::adapters::bundle::BundleError;

const OVERRIDE_PATH: &str = "profiles/vendors/moergo/all_kconfig.toml";
const EMBEDDED_KCONFIG_TOML: &str =
    include_str!("../../../profiles/vendors/moergo/all_kconfig.toml");

#[derive(Debug, Clone, Default)]
pub struct MoergoKconfigMap {
    alias_to_canonical: BTreeMap<String, String>,
    canonical_to_alias: BTreeMap<String, String>,
}

impl MoergoKconfigMap {
    pub fn canonical_for_alias(&self, alias: &str) -> Option<&str> {
        self.alias_to_canonical.get(alias).map(|s| s.as_str())
    }

    pub fn alias_for_canonical(&self, canonical: &str) -> Option<&str> {
        self.canonical_to_alias.get(canonical).map(|s| s.as_str())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct KeymapToml {
    keymap: Option<KeymapSection>,
}

#[derive(Debug, Clone, Deserialize)]
struct KeymapSection {
    #[serde(default)]
    kconfig_options: BTreeMap<String, KconfigOption>,
}

#[derive(Debug, Clone, Deserialize)]
struct KconfigOption {
    name: Option<String>,
}

/// Load the alias ↔ canonical KConfig mapping, preferring an on-disk override.
pub fn load_kconfig_map() -> Result<MoergoKconfigMap, BundleError> {
    let source = load_overridable_toml()?;
    parse_kconfig_map(&source)
}

fn load_overridable_toml() -> Result<String, BundleError> {
    let override_path = Path::new(OVERRIDE_PATH);
    if override_path.exists() {
        Ok(fs::read_to_string(override_path)?)
    } else {
        Ok(EMBEDDED_KCONFIG_TOML.to_string())
    }
}

fn parse_kconfig_map(input: &str) -> Result<MoergoKconfigMap, BundleError> {
    let parsed: KeymapToml = toml::from_str(input)?;
    let mut map = MoergoKconfigMap::default();
    let Some(section) = parsed.keymap else {
        return Ok(map);
    };

    for (alias, option) in section.kconfig_options {
        let Some(canonical) = option.name else {
            continue;
        };
        let alias = alias.trim();
        let canonical = canonical.trim();
        if alias.is_empty() || canonical.is_empty() {
            continue;
        }
        map.alias_to_canonical
            .entry(alias.to_string())
            .or_insert_with(|| canonical.to_string());
        map.canonical_to_alias
            .entry(canonical.to_string())
            .or_insert_with(|| alias.to_string());
    }

    Ok(map)
}
