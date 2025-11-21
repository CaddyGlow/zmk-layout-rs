use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use thiserror::Error;

use crate::{
    adapters::standard::{AdapterLayout, render_layout_with_template},
    dts::DtsDocument,
};

const DEFAULT_FORMAT_VERSION: &str = "layout-bundle/2025-02-01";

/// Describes an adapter-friendly bundle that keeps layout data plus build/context metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutBundle {
    pub format_version: String,
    #[serde(default)]
    pub metadata: BundleMetadata,
    pub layout: AdapterLayout,
    #[serde(default)]
    pub overlays: BundleOverlays,
    #[serde(default)]
    pub symbols: BundleSymbols,
    #[serde(default)]
    pub targets: Vec<BundleTarget>,
    #[serde(default)]
    pub sources: BTreeMap<String, BundleSource>,
}

impl Default for LayoutBundle {
    fn default() -> Self {
        Self {
            format_version: DEFAULT_FORMAT_VERSION.to_string(),
            metadata: BundleMetadata::default(),
            layout: AdapterLayout::default(),
            overlays: BundleOverlays::default(),
            symbols: BundleSymbols::default(),
            targets: Vec::new(),
            sources: BTreeMap::new(),
        }
    }
}

impl LayoutBundle {
    /// Validate bundle structure (format version, target ids, overlay references).
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.format_version != DEFAULT_FORMAT_VERSION {
            return Err(BundleError::Validation(format!(
                "unsupported format_version `{}` (expected `{DEFAULT_FORMAT_VERSION}`)",
                self.format_version
            )));
        }
        let mut ids = HashSet::new();
        for target in &self.targets {
            if !ids.insert(&target.id) {
                return Err(BundleError::Validation(format!(
                    "duplicate target id `{}`",
                    target.id
                )));
            }
            if target
                .template
                .as_ref()
                .map(|path| path.trim().is_empty())
                .unwrap_or(true)
            {
                return Err(BundleError::Validation(format!(
                    "target `{}` is missing a template path",
                    target.id
                )));
            }
            for overlay in &target.overlays {
                if !self.overlays.contains(overlay) {
                    return Err(BundleError::Validation(format!(
                        "target `{}` references unknown overlay `{}`",
                        target.id, overlay
                    )));
                }
            }
            for define in &target.defines {
                if !self.symbols.defines.contains_key(define) {
                    return Err(BundleError::Validation(format!(
                        "target `{}` references unknown define `{}`",
                        target.id, define
                    )));
                }
            }
        }
        Ok(())
    }

    /// Build a bundle from a MoErgo JSON payload.
    pub fn from_moergo_str(json: &str) -> Result<Self, BundleError> {
        crate::adapters::moergo::adapter::import_bundle_from_str(json)
    }

    /// Serialize the bundle back into a MoErgo JSON payload.
    pub fn to_moergo_json(&self) -> Result<String, BundleError> {
        crate::adapters::moergo::adapter::export_bundle_to_moergo_json(self)
    }

    /// Render a target into DTS text using the referenced template.
    pub fn render_target(
        &self,
        target_id: &str,
        template_override: Option<&Path>,
    ) -> Result<String, BundleError> {
        let target = self
            .targets
            .iter()
            .find(|target| target.id == target_id)
            .ok_or_else(|| BundleError::TargetNotFound(target_id.to_string()))?;
        let template_path = if let Some(path) = template_override {
            PathBuf::from(path)
        } else {
            target
                .template
                .as_ref()
                .map(PathBuf::from)
                .ok_or(BundleError::MissingTemplate)?
        };
        let template_source = fs::read_to_string(&template_path)?;
        let mut layout = self.layout.clone();
        apply_overlays_and_symbols(
            &self.metadata,
            &self.overlays,
            &self.symbols,
            target,
            &mut layout,
        )?;
        let rendered = render_layout_with_template(&layout, &template_source)?;
        DtsDocument::parse_str(&rendered)?;
        Ok(rendered)
    }

    pub fn from_moergo_file(path: impl AsRef<Path>) -> Result<Self, BundleError> {
        crate::adapters::moergo::adapter::import_bundle_from_file(path)
    }

    pub fn write_json(&self, path: impl AsRef<Path>) -> Result<(), BundleError> {
        let payload = serde_json::to_string_pretty(self)?;
        fs::write(path, payload)?;
        Ok(())
    }

    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self, BundleError> {
        let payload = fs::read_to_string(path)?;
        let bundle: Self = serde_json::from_str(&payload)?;
        bundle.validate()?;
        Ok(bundle)
    }
}

fn apply_overlays_and_symbols(
    metadata: &BundleMetadata,
    overlays: &BundleOverlays,
    symbols: &BundleSymbols,
    target: &BundleTarget,
    layout: &mut AdapterLayout,
) -> Result<(), BundleError> {
    let overlay_set: HashSet<&str> = target.overlays.iter().map(|name| name.as_str()).collect();
    let overlay_selected = |name: &str| overlay_set.contains(name);

    if layout.metadata.title.is_none() {
        layout.metadata.title = metadata.title.clone();
    }
    if layout.metadata.description.is_none() {
        layout.metadata.description = metadata.description.clone();
    }
    let extras = &mut layout.metadata.extras;
    for (key, value) in &metadata.extras {
        extras.entry(key.clone()).or_insert_with(|| value.clone());
    }
    if overlay_selected("custom_devicetree") {
        if let Some(custom) = overlays.custom_devicetree.as_ref() {
            extras.insert("custom_devicetree".into(), Value::String(custom.clone()));
        }
    }
    if overlay_selected("custom_behaviors") {
        if let Some(custom) = overlays.custom_behaviors.as_ref() {
            extras.insert(
                "custom_defined_behaviors".into(),
                Value::String(custom.clone()),
            );
        }
    }
    if overlay_selected("custom_macros") {
        if let Some(custom) = overlays.custom_macros.as_ref() {
            extras.insert(
                "custom_defined_macros".into(),
                Value::String(custom.clone()),
            );
        }
    }
    if overlay_selected("input_listeners") {
        if let Some(listeners) = overlays.input_listeners.as_ref() {
            extras.insert("input_listeners".into(), Value::String(listeners.clone()));
        }
    }
    if let Some(header) = overlays.fragments.get("key_position_header") {
        if overlay_selected("fragments.key_position_header") {
            extras.insert("key_position_header".into(), Value::String(header.clone()));
        }
    }
    let mut includes = symbols.includes.clone();
    includes.extend(target.includes.clone());
    if !includes.is_empty() {
        extras.insert("includes".into(), Value::String(includes.join("\n")));
    }
    for (name, value) in &symbols.template_vars {
        extras.insert(name.clone(), value.clone());
    }
    if let Some(rendered) = render_defines_for_target(symbols, target)? {
        extras.insert("defines".into(), Value::String(rendered));
    }
    Ok(())
}

fn render_defines_for_target(
    symbols: &BundleSymbols,
    target: &BundleTarget,
) -> Result<Option<String>, BundleError> {
    if target.defines.is_empty() {
        return Ok(None);
    }
    let mut lines = Vec::new();
    for name in &target.defines {
        let value = symbols.defines.get(name).ok_or_else(|| {
            BundleError::Validation(format!(
                "target `{}` references unknown define `{name}`",
                target.id
            ))
        })?;
        lines.push(render_define(name, value));
    }
    Ok(Some(lines.join("\n")))
}

fn render_define(name: &str, value: &Value) -> String {
    let rendered_value = match value {
        Value::Null => None,
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(num) => Some(num.to_string()),
        Value::String(text) => Some(text.to_string()),
        other => Some(other.to_string()),
    };
    match rendered_value {
        Some(val) if val.is_empty() => format!("#define {name}"),
        Some(val) => format!("#define {name} {val}"),
        None => format!("#define {name}"),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub keyboard: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub extras: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleOverlays {
    pub custom_devicetree: Option<String>,
    pub custom_behaviors: Option<String>,
    pub custom_macros: Option<String>,
    pub input_listeners: Option<String>,
    #[serde(default)]
    pub fragments: BTreeMap<String, String>,
}

impl BundleOverlays {
    fn contains(&self, name: &str) -> bool {
        match name {
            "custom_devicetree" => self.custom_devicetree.is_some(),
            "custom_behaviors" => self.custom_behaviors.is_some(),
            "custom_macros" => self.custom_macros.is_some(),
            "input_listeners" => self.input_listeners.is_some(),
            fragment if fragment.starts_with("fragments.") => {
                let key = fragment.trim_start_matches("fragments.");
                self.fragments.contains_key(key)
            }
            _ => false,
        }
    }

    pub fn default_overlay_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if self.custom_devicetree.is_some() {
            names.push("custom_devicetree".to_string());
        }
        if self.custom_behaviors.is_some() {
            names.push("custom_behaviors".to_string());
        }
        if self.custom_macros.is_some() {
            names.push("custom_macros".to_string());
        }
        if self.input_listeners.is_some() {
            names.push("input_listeners".to_string());
        }
        for key in self.fragments.keys() {
            names.push(format!("fragments.{key}"));
        }
        names
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleSymbols {
    #[serde(default)]
    pub defines: BTreeMap<String, Value>,
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default)]
    pub search_paths: Vec<String>,
    #[serde(default)]
    pub template_vars: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleTarget {
    pub id: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub firmware: Option<BundleFirmware>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub overlays: Vec<String>,
    #[serde(default)]
    pub defines: Vec<String>,
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default)]
    pub output: Option<BundleOutput>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleFirmware {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub board: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleOutput {
    pub format: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundleSource {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub schema_version: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Error, Debug)]
pub enum BundleError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("template error: {0}")]
    Template(#[from] crate::adapters::standard::TemplateError),
    #[error("DTS parse error: {0}")]
    Dts(#[from] crate::dts::DtsError),
    #[error("layout parse error: {0}")]
    Layout(#[from] crate::tokenizer::LayoutError),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("target `{0}` not found")]
    TargetNotFound(String),
    #[error("target is missing a template path")]
    MissingTemplate,
}
