//! Keyboard profile loader for the TOML schema described in `docs/keyboard_profiles.md`.

use rust_embed::RustEmbed;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;
use toml::Value as TomlValue;

/// Embedded keyboard profiles bundled in the binary.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../profiles/keyboards/"]
pub struct EmbeddedKeyboardProfiles;

/// Embedded firmware profiles bundled in the binary.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../profiles/firmwares/"]
pub struct EmbeddedFirmwareProfiles;

/// Embedded vendor profiles bundled in the binary.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../profiles/vendors/"]
pub struct EmbeddedVendorProfiles;

/// Convenience type used for arbitrary TOML tables we want to retain.
pub type ProfileProperties = BTreeMap<String, TomlValue>;

/// Fully parsed keyboard profile document.
#[derive(Debug, Clone)]
pub struct KeyboardProfileDoc {
    pub keyboard: String,
    pub version: u32,
    pub metadata: MetadataSection,
    pub hardware: HardwareSection,
    pub firmware: FirmwareSection,
    pub layout: LayoutSection,
    pub behaviors: Vec<BehaviorEntry>,
    pub combos: Vec<ComboEntry>,
    pub macros: Vec<MacroEntry>,
}

impl KeyboardProfileDoc {
    /// Load a keyboard profile from TOML text.
    pub fn from_toml_str(input: &str) -> Result<Self, ProfileError> {
        let raw: RawKeyboardProfileDoc = toml::from_str(input).map_err(ProfileError::Parse)?;
        raw.try_into()
    }

    /// Load a keyboard profile from disk.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ProfileError> {
        let path_ref = path.as_ref();
        let contents = fs::read_to_string(path_ref).map_err(|source| ProfileError::ReadFile {
            path: path_ref.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&contents)
    }

    /// Load a keyboard profile by name, checking filesystem first, then embedded profiles.
    ///
    /// This allows external files to override embedded profiles.
    /// The name should be just the profile name without path or extension (e.g., "glove80").
    ///
    /// Search order:
    /// 1. `profiles/keyboards/{name}/profile.toml` in filesystem
    /// 2. `profiles/keyboards/{name}/{name}.toml` in filesystem
    /// 3. `profiles/keyboards/{name}.toml` in filesystem
    /// 4. Embedded profile (`{name}/profile.toml`, `{name}/{name}.toml`, or `{name}.toml`)
    pub fn load(name: &str) -> Result<Self, ProfileError> {
        // Try filesystem first (allows override)
        for candidate in filesystem_profile_candidates(name) {
            if candidate.exists() {
                return Self::from_file(&candidate);
            }
        }

        // Fall back to embedded profile
        for candidate in embedded_profile_candidates(name) {
            if let Some(embedded) = EmbeddedKeyboardProfiles::get(&candidate) {
                let contents = std::str::from_utf8(embedded.data.as_ref()).map_err(|_| {
                    ProfileError::Validation("embedded profile is not valid UTF-8".into())
                })?;
                return Self::from_toml_str(contents);
            }
        }

        Err(ProfileError::NotFound(name.to_string()))
    }

    /// List all available keyboard profiles (both embedded and filesystem).
    pub fn list_available() -> Vec<String> {
        let mut profiles = BTreeSet::new();

        // Add embedded profiles
        for file in EmbeddedKeyboardProfiles::iter() {
            let path = Path::new(file.as_ref());
            if let Some(name) = profile_name_from_path(path) {
                profiles.insert(name);
            }
        }

        // Add filesystem profiles (may override embedded)
        let root = Path::new("profiles/keyboards");
        if root.exists() {
            collect_profile_names(root, &mut profiles);
        }

        profiles.into_iter().collect()
    }

    /// Try to detect which profile matches a rendered DTS layout based on profile hints.
    pub fn detect_from_rendered(rendered: &str) -> Vec<Self> {
        KeyboardProfileDoc::list_available()
            .into_iter()
            .filter_map(|name| KeyboardProfileDoc::load(&name).ok())
            .filter(|profile| profile.layout.detection.matches(rendered))
            .collect()
    }
}

pub(crate) fn filesystem_profile_candidates(name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let root = PathBuf::from("profiles/keyboards");
    push_unique_path(&mut candidates, root.join(name).join("profile.toml"));
    push_unique_path(
        &mut candidates,
        root.join(name).join(format!("{name}.toml")),
    );
    push_unique_path(&mut candidates, root.join(format!("{name}.toml")));
    candidates
}

pub(crate) fn embedded_profile_candidates(name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_string(&mut candidates, format!("{name}/profile.toml"));
    push_unique_string(&mut candidates, format!("{name}/{name}.toml"));
    push_unique_string(&mut candidates, format!("{name}.toml"));
    candidates
}

/// Load kconfig_options from a vendor's `all_kconfig.toml`.
///
/// Searches filesystem first (`profiles/vendors/{vendor}/all_kconfig.toml`),
/// then falls back to embedded vendor profiles.  Returns `None` when no map
/// is found for the given vendor.
pub fn load_vendor_kconfig_options(vendor: &str) -> Option<BTreeMap<String, TomlValue>> {
    let filename = format!("{}/all_kconfig.toml", vendor.to_lowercase());

    // Try filesystem first (allows override)
    let fs_path = PathBuf::from("profiles/vendors").join(&filename);
    let contents = if fs_path.exists() {
        fs::read_to_string(&fs_path).ok()?
    } else {
        let embedded = EmbeddedVendorProfiles::get(&filename)?;
        std::str::from_utf8(embedded.data.as_ref()).ok()?.to_string()
    };

    let root: TomlValue = toml::from_str(&contents).ok()?;
    let kconfig_options = root
        .as_table()?
        .get("keymap")?
        .as_table()?
        .get("kconfig_options")?
        .as_table()?;

    Some(
        kconfig_options
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    )
}

fn collect_profile_names(root: &Path, profiles: &mut BTreeSet<String>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(path);
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            if let Some(name) = profile_name_from_path(&path) {
                profiles.insert(name);
            }
        }
    }
}

fn profile_name_from_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    if !ext.eq_ignore_ascii_case("toml") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    if stem.eq_ignore_ascii_case("profile") {
        return path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .map(|name| name.to_string());
    }
    Some(stem.to_string())
}

fn push_unique_path(vec: &mut Vec<PathBuf>, value: PathBuf) {
    if !vec.contains(&value) {
        vec.push(value);
    }
}

fn push_unique_string(vec: &mut Vec<String>, value: String) {
    if !vec.iter().any(|existing| existing == &value) {
        vec.push(value);
    }
}

/// Document metadata identifying the keyboard.
#[derive(Debug, Clone)]
pub struct MetadataSection {
    pub name: String,
    pub vendor: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub tags: Vec<String>,
    pub extras: ProfileProperties,
}

/// Hardware configuration details.
#[derive(Debug, Clone)]
pub struct HardwareSection {
    pub key_count: u32,
    pub is_split: bool,
    pub controllers: Vec<String>,
    pub boards: Vec<HardwareBoard>,
    pub flash: Vec<HardwareFlash>,
    pub build_defaults: HardwareBuildDefaults,
    pub extras: ProfileProperties,
}

/// Board entry used when a keyboard exposes multiple device ids.
#[derive(Debug, Clone)]
pub struct HardwareBoard {
    pub id: String,
    pub role: Option<String>,
    pub variant: Option<String>,
    pub extras: ProfileProperties,
}

/// Flash strategy description.
#[derive(Debug, Clone)]
pub struct HardwareFlash {
    pub method: Option<String>,
    pub device_query: Option<String>,
    pub mount_timeout: Option<u32>,
    pub copy_timeout: Option<u32>,
    pub sync_after_copy: Option<bool>,
    pub extras: ProfileProperties,
}

/// Default build inputs shared by most targets.
#[derive(Debug, Clone, Default)]
pub struct HardwareBuildDefaults {
    pub cmake: ProfileProperties,
    pub env: ProfileProperties,
    pub kconfig: ProfileProperties,
    pub extras: ProfileProperties,
}

/// Firmware definitions referenced by the manifest/CLI.
#[derive(Debug, Clone)]
pub struct FirmwareSection {
    pub default: String,
    pub versions: BTreeMap<String, FirmwareVersion>,
}

/// Individual firmware release definition.
#[derive(Debug, Clone)]
pub struct FirmwareVersion {
    pub id: String,
    pub repository: String,
    pub branch: String,
    pub channel: Option<String>,
    pub notes: Option<String>,
    pub properties: ProfileProperties,
}

/// Layout configuration used by adapters and generators.
#[derive(Debug, Clone)]
pub struct LayoutSection {
    pub template: String,
    pub formatting: LayoutFormatting,
    pub keymap: LayoutKeymap,
    pub renderers: BTreeMap<String, LayoutRenderer>,
    pub detection: LayoutDetection,
    pub extras: ProfileProperties,
}

/// Formatting helpers describing key positions/rows.
#[derive(Debug, Clone)]
pub struct LayoutFormatting {
    pub key_gap: Option<String>,
    pub base_indent: Option<String>,
    pub rows: Vec<LayoutFormattingRow>,
    pub extras: ProfileProperties,
}

/// Formatting row entry containing the ordered key positions.
#[derive(Debug, Clone)]
pub struct LayoutFormattingRow {
    pub keys: Vec<i32>,
    pub extras: ProfileProperties,
}

/// Metadata describing the generated keymap artifacts.
#[derive(Debug, Clone, Default)]
pub struct LayoutKeymap {
    pub header_includes: Vec<String>,
    pub properties: ProfileProperties,
}

/// Alternate renderers (ASCII templates, etc.).
#[derive(Debug, Clone)]
pub struct LayoutRenderer {
    pub template: String,
    pub description: Option<String>,
    pub extras: ProfileProperties,
}

/// Heuristics used to detect whether a rendered DTS belongs to this profile.
#[derive(Debug, Clone, Default)]
pub struct LayoutDetection {
    pub markers: Vec<String>,
    pub regex: Vec<String>,
    pub extras: ProfileProperties,
}

impl LayoutFormattingRow {
    /// Render the row positions as a fixed-width ASCII string for debugging/tests.
    pub fn ascii_art(&self) -> String {
        const EMPTY: &str = "  .";
        self.keys
            .iter()
            .map(|value| {
                if *value < 0 {
                    EMPTY.to_string()
                } else {
                    format!("{:>3}", value)
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl LayoutDetection {
    /// Returns true if any configured marker or regex matches the rendered DTS text.
    pub fn matches(&self, rendered: &str) -> bool {
        self.markers.iter().any(|marker| rendered.contains(marker))
            || self
                .regex
                .iter()
                .filter_map(|pattern| regex::Regex::new(pattern).ok())
                .any(|re| re.is_match(rendered))
    }
}

impl LayoutKeymap {
    /// Fetch the optional key position header snippet from the profile.
    pub fn key_position_header(&self) -> Option<&str> {
        string_property(&self.properties, "key_position_header")
    }

    /// Fetch the optional system behaviors block from the profile.
    pub fn system_behaviors_dts(&self) -> Option<&str> {
        string_property(&self.properties, "system_behaviors_dts")
    }
}

/// Behavior metadata exposed by keyboards.
#[derive(Debug, Clone)]
pub struct BehaviorEntry {
    pub id: String,
    pub code: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub origin: Option<String>,
    pub expected_params: Option<u32>,
    pub is_macro_control_behavior: bool,
    pub params: Vec<String>,
    pub extras: ProfileProperties,
}

/// Combo metadata describing bindings/layers.
#[derive(Debug, Clone)]
pub struct ComboEntry {
    pub name: String,
    pub key_positions: Vec<i32>,
    pub binding: Option<String>,
    pub layers: Vec<String>,
    pub timeout_ms: Option<u32>,
    pub extras: ProfileProperties,
}

/// Macro metadata shared with layout tooling.
#[derive(Debug, Clone)]
pub struct MacroEntry {
    pub name: String,
    pub description: Option<String>,
    pub bindings: Vec<String>,
    pub extras: ProfileProperties,
}

/// Errors raised while loading keyboard profiles.
#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("failed to read profile {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse profile: {0}")]
    Parse(toml::de::Error),
    #[error("keyboard profile missing `{0}`")]
    MissingField(String),
    #[error("keyboard profile validation failed: {0}")]
    Validation(String),
    #[error("keyboard profile `{0}` not found in embedded profiles or filesystem")]
    NotFound(String),
}

#[derive(Debug, Deserialize)]
struct RawKeyboardProfileDoc {
    keyboard: Option<String>,
    version: Option<u32>,
    metadata: Option<RawMetadataSection>,
    hardware: Option<RawHardwareSection>,
    firmware: Option<RawFirmwareSection>,
    layout: Option<RawLayoutSection>,
    #[serde(default)]
    behaviors: Vec<RawBehaviorEntry>,
    #[serde(default)]
    combos: Vec<RawComboEntry>,
    #[serde(default)]
    macros: Vec<RawMacroEntry>,
}

impl TryFrom<RawKeyboardProfileDoc> for KeyboardProfileDoc {
    type Error = ProfileError;

    fn try_from(raw: RawKeyboardProfileDoc) -> Result<Self, Self::Error> {
        let keyboard = require_non_empty_string(raw.keyboard, "keyboard")?;
        let version = require_field(raw.version, "version")?;
        let metadata = raw
            .metadata
            .ok_or_else(|| missing("metadata"))?
            .try_into()?;
        let hardware = raw
            .hardware
            .ok_or_else(|| missing("hardware"))?
            .try_into()?;
        let firmware = raw
            .firmware
            .ok_or_else(|| missing("firmware"))?
            .try_into()?;
        let layout = raw.layout.ok_or_else(|| missing("layout"))?.try_into()?;
        let behaviors = raw
            .behaviors
            .into_iter()
            .map(BehaviorEntry::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let combos = raw
            .combos
            .into_iter()
            .map(ComboEntry::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let macros = raw
            .macros
            .into_iter()
            .map(MacroEntry::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(KeyboardProfileDoc {
            keyboard,
            version,
            metadata,
            hardware,
            firmware,
            layout,
            behaviors,
            combos,
            macros,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawMetadataSection {
    name: Option<String>,
    vendor: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawMetadataSection> for MetadataSection {
    type Error = ProfileError;

    fn try_from(raw: RawMetadataSection) -> Result<Self, Self::Error> {
        Ok(MetadataSection {
            name: require_non_empty_string(raw.name, "metadata.name")?,
            vendor: require_non_empty_string(raw.vendor, "metadata.vendor")?,
            description: raw.description,
            homepage: raw.homepage,
            tags: raw.tags,
            extras: raw.extras,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawHardwareSection {
    key_count: Option<u32>,
    is_split: Option<bool>,
    #[serde(default)]
    controllers: Vec<String>,
    #[serde(default)]
    boards: Vec<RawHardwareBoard>,
    #[serde(default)]
    flash: Vec<RawHardwareFlash>,
    #[serde(default)]
    build_defaults: Option<RawHardwareBuildDefaults>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawHardwareSection> for HardwareSection {
    type Error = ProfileError;

    fn try_from(raw: RawHardwareSection) -> Result<Self, Self::Error> {
        let boards = raw
            .boards
            .into_iter()
            .map(HardwareBoard::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let flash = raw.flash.into_iter().map(HardwareFlash::from).collect();
        let build_defaults = raw
            .build_defaults
            .map(HardwareBuildDefaults::from)
            .unwrap_or_default();
        Ok(HardwareSection {
            key_count: require_field(raw.key_count, "hardware.key_count")?,
            is_split: require_field(raw.is_split, "hardware.is_split")?,
            controllers: raw.controllers,
            boards,
            flash,
            build_defaults,
            extras: raw.extras,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawHardwareBoard {
    id: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    variant: Option<String>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawHardwareBoard> for HardwareBoard {
    type Error = ProfileError;

    fn try_from(raw: RawHardwareBoard) -> Result<Self, Self::Error> {
        Ok(HardwareBoard {
            id: require_non_empty_string(raw.id, "hardware.boards.id")?,
            role: raw.role,
            variant: raw.variant,
            extras: raw.extras,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawHardwareFlash {
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    device_query: Option<String>,
    #[serde(default)]
    mount_timeout: Option<u32>,
    #[serde(default)]
    copy_timeout: Option<u32>,
    #[serde(default)]
    sync_after_copy: Option<bool>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl From<RawHardwareFlash> for HardwareFlash {
    fn from(raw: RawHardwareFlash) -> Self {
        HardwareFlash {
            method: raw.method,
            device_query: raw.device_query,
            mount_timeout: raw.mount_timeout,
            copy_timeout: raw.copy_timeout,
            sync_after_copy: raw.sync_after_copy,
            extras: raw.extras,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawHardwareBuildDefaults {
    #[serde(default)]
    cmake: Option<ProfileProperties>,
    #[serde(default)]
    env: Option<ProfileProperties>,
    #[serde(default)]
    kconfig: Option<ProfileProperties>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl From<RawHardwareBuildDefaults> for HardwareBuildDefaults {
    fn from(raw: RawHardwareBuildDefaults) -> Self {
        HardwareBuildDefaults {
            cmake: raw.cmake.unwrap_or_default(),
            env: raw.env.unwrap_or_default(),
            kconfig: raw.kconfig.unwrap_or_default(),
            extras: raw.extras,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawFirmwareSection {
    default: Option<String>,
    #[serde(default)]
    versions: BTreeMap<String, RawFirmwareVersion>,
}

impl TryFrom<RawFirmwareSection> for FirmwareSection {
    type Error = ProfileError;

    fn try_from(raw: RawFirmwareSection) -> Result<Self, Self::Error> {
        if raw.versions.is_empty() {
            return Err(ProfileError::Validation(
                "firmware.versions cannot be empty".into(),
            ));
        }
        let mut versions = BTreeMap::new();
        for (id, entry) in raw.versions {
            versions.insert(id.clone(), entry.into_version(&id)?);
        }
        let default = require_non_empty_string(raw.default, "firmware.default")?;
        if !versions.contains_key(&default) {
            return Err(ProfileError::Validation(format!(
                "firmware.default `{}` is not present under firmware.versions",
                default
            )));
        }
        Ok(FirmwareSection { default, versions })
    }
}

#[derive(Debug, Deserialize)]
struct RawFirmwareVersion {
    repository: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(flatten)]
    properties: ProfileProperties,
}

impl RawFirmwareVersion {
    fn into_version(self, id: &str) -> Result<FirmwareVersion, ProfileError> {
        Ok(FirmwareVersion {
            id: id.to_string(),
            repository: require_non_empty_string(
                self.repository,
                &format!("firmware.versions.{}.repository", id),
            )?,
            branch: require_non_empty_string(
                self.branch,
                &format!("firmware.versions.{}.branch", id),
            )?,
            channel: self.channel,
            notes: self.notes,
            properties: self.properties,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawLayoutSection {
    template: Option<String>,
    formatting: Option<RawLayoutFormatting>,
    #[serde(default)]
    keymap: Option<RawLayoutKeymap>,
    #[serde(default)]
    renderers: BTreeMap<String, RawLayoutRenderer>,
    #[serde(default)]
    detection: Option<RawLayoutDetection>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawLayoutSection> for LayoutSection {
    type Error = ProfileError;

    fn try_from(raw: RawLayoutSection) -> Result<Self, Self::Error> {
        let formatting = raw
            .formatting
            .ok_or_else(|| missing("layout.formatting"))?
            .try_into()?;
        let keymap = raw.keymap.map(LayoutKeymap::from).unwrap_or_default();
        let mut renderers = BTreeMap::new();
        for (name, renderer) in raw.renderers {
            renderers.insert(name, renderer.try_into()?);
        }
        Ok(LayoutSection {
            template: require_non_empty_string(raw.template, "layout.template")?,
            formatting,
            keymap,
            renderers,
            detection: raw
                .detection
                .map(LayoutDetection::try_from)
                .transpose()?
                .unwrap_or_default(),
            extras: raw.extras,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawLayoutFormatting {
    #[serde(default)]
    key_gap: Option<String>,
    #[serde(default)]
    base_indent: Option<String>,
    rows: Option<Vec<RawLayoutFormattingRow>>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawLayoutFormatting> for LayoutFormatting {
    type Error = ProfileError;

    fn try_from(raw: RawLayoutFormatting) -> Result<Self, Self::Error> {
        let rows = raw.rows.ok_or_else(|| missing("layout.formatting.rows"))?;
        if rows.is_empty() {
            return Err(ProfileError::Validation(
                "layout.formatting.rows must contain at least one entry".into(),
            ));
        }
        let rows = rows
            .into_iter()
            .map(LayoutFormattingRow::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(LayoutFormatting {
            key_gap: raw.key_gap,
            base_indent: raw.base_indent,
            rows,
            extras: raw.extras,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawLayoutFormattingRow {
    keys: Option<Vec<i32>>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

#[derive(Debug, Deserialize)]
struct RawLayoutDetection {
    #[serde(default)]
    markers: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawLayoutDetection> for LayoutDetection {
    type Error = ProfileError;

    fn try_from(raw: RawLayoutDetection) -> Result<Self, Self::Error> {
        if raw.markers.is_empty() && raw.regex.is_empty() {
            return Err(ProfileError::Validation(
                "layout.detection must include at least one marker or regex".into(),
            ));
        }
        Ok(LayoutDetection {
            markers: raw.markers,
            regex: raw.regex,
            extras: raw.extras,
        })
    }
}

impl TryFrom<RawLayoutFormattingRow> for LayoutFormattingRow {
    type Error = ProfileError;

    fn try_from(raw: RawLayoutFormattingRow) -> Result<Self, Self::Error> {
        let keys = raw
            .keys
            .ok_or_else(|| missing("layout.formatting.rows.keys"))?;
        if keys.is_empty() {
            return Err(ProfileError::Validation(
                "layout.formatting.rows entries must define at least one key".into(),
            ));
        }
        Ok(LayoutFormattingRow {
            keys,
            extras: raw.extras,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawLayoutKeymap {
    #[serde(default)]
    header_includes: Vec<String>,
    #[serde(flatten)]
    properties: ProfileProperties,
}

impl From<RawLayoutKeymap> for LayoutKeymap {
    fn from(raw: RawLayoutKeymap) -> Self {
        LayoutKeymap {
            header_includes: raw.header_includes,
            properties: raw.properties,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawLayoutRenderer {
    template: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawLayoutRenderer> for LayoutRenderer {
    type Error = ProfileError;

    fn try_from(raw: RawLayoutRenderer) -> Result<Self, Self::Error> {
        Ok(LayoutRenderer {
            template: require_non_empty_string(raw.template, "layout.renderers.template")?,
            description: raw.description,
            extras: raw.extras,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawBehaviorEntry {
    id: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default)]
    expected_params: Option<u32>,
    #[serde(default)]
    is_macro_control_behavior: bool,
    #[serde(default)]
    params: Vec<String>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawBehaviorEntry> for BehaviorEntry {
    type Error = ProfileError;

    fn try_from(raw: RawBehaviorEntry) -> Result<Self, Self::Error> {
        Ok(BehaviorEntry {
            id: require_non_empty_string(raw.id, "behaviors.id")?,
            code: raw.code,
            name: raw.name,
            description: raw.description,
            url: raw.url,
            origin: raw.origin,
            expected_params: raw.expected_params,
            is_macro_control_behavior: raw.is_macro_control_behavior,
            params: raw.params,
            extras: raw.extras,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawComboEntry {
    name: Option<String>,
    #[serde(default)]
    key_positions: Vec<i32>,
    #[serde(default)]
    binding: Option<String>,
    #[serde(default)]
    layers: Vec<String>,
    #[serde(default)]
    timeout_ms: Option<u32>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawComboEntry> for ComboEntry {
    type Error = ProfileError;

    fn try_from(raw: RawComboEntry) -> Result<Self, Self::Error> {
        if raw.key_positions.is_empty() {
            return Err(ProfileError::Validation(
                "combos entries must define key_positions".into(),
            ));
        }
        Ok(ComboEntry {
            name: require_non_empty_string(raw.name, "combos.name")?,
            key_positions: raw.key_positions,
            binding: raw.binding,
            layers: raw.layers,
            timeout_ms: raw.timeout_ms,
            extras: raw.extras,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawMacroEntry {
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    bindings: Vec<String>,
    #[serde(flatten)]
    extras: ProfileProperties,
}

impl TryFrom<RawMacroEntry> for MacroEntry {
    type Error = ProfileError;

    fn try_from(raw: RawMacroEntry) -> Result<Self, Self::Error> {
        Ok(MacroEntry {
            name: require_non_empty_string(raw.name, "macros.name")?,
            description: raw.description,
            bindings: raw.bindings,
            extras: raw.extras,
        })
    }
}

fn missing(field: &str) -> ProfileError {
    ProfileError::MissingField(field.to_string())
}

fn require_field<T>(value: Option<T>, field: &str) -> Result<T, ProfileError> {
    value.ok_or_else(|| missing(field))
}

fn require_non_empty_string(value: Option<String>, field: &str) -> Result<String, ProfileError> {
    let text = require_field(value, field)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(ProfileError::Validation(format!(
            "{} cannot be empty",
            field
        )));
    }
    Ok(trimmed.to_string())
}

fn string_property<'a>(map: &'a ProfileProperties, key: &str) -> Option<&'a str> {
    map.get(key).and_then(|value| value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile_path(name: &str) -> String {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        format!("{manifest_dir}/../../profiles/keyboards/{name}")
    }

    #[test]
    fn parses_glove80_profile() {
        let doc = KeyboardProfileDoc::from_file(&test_profile_path("glove80/profile.toml"))
            .expect("profile");
        assert_eq!(doc.keyboard, "glove80");
        assert_eq!(doc.version, 1);
        assert_eq!(doc.metadata.name, "MoErgo Glove80");
        assert_eq!(doc.hardware.key_count, 80);
        assert_eq!(doc.layout.formatting.rows.len(), 6);
        assert!(doc
            .layout
            .keymap
            .properties
            .contains_key("system_behaviors_dts"));
    }

    #[test]
    fn errors_on_missing_keyboard() {
        let text = r#"
version = 1

[metadata]
name = "Test"
vendor = "Acme"

[hardware]
key_count = 1
is_split = false

[firmware]
default = "stable"
[firmware.versions.stable]
repository = "org/repo"
branch = "main"

[layout]
template = "layout.dtsi"
[[layout.formatting.rows]]
keys = [0]
"#;
        let err = KeyboardProfileDoc::from_toml_str(text).expect_err("error");
        match err {
            ProfileError::MissingField(field) => assert_eq!(field, "keyboard"),
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn errors_when_rows_missing() {
        let text = r#"
keyboard = "demo"
version = 1

[metadata]
name = "Demo"
vendor = "Acme"

[hardware]
key_count = 1
is_split = false

[firmware]
default = "stable"
[firmware.versions.stable]
repository = "org/repo"
branch = "main"

[layout]
template = "layout.dtsi"
[layout.formatting]
"#;
        let err = KeyboardProfileDoc::from_toml_str(text).expect_err("error");
        assert!(
            matches!(err, ProfileError::MissingField(field) if field == "layout.formatting.rows")
        );
    }

    #[test]
    fn preserves_extra_tables() {
        let doc = KeyboardProfileDoc::from_file(&test_profile_path("glove80/profile.toml"))
            .expect("profile");
        assert!(doc.hardware.build_defaults.extras.contains_key("board"));
        assert!(doc
            .layout
            .keymap
            .properties
            .contains_key("key_position_header"));
    }

    #[test]
    fn formatting_rows_render_ascii() {
        let doc = KeyboardProfileDoc::from_file(&test_profile_path("glove80/profile.toml"))
            .expect("profile");
        let ascii: Vec<String> = doc
            .layout
            .formatting
            .rows
            .iter()
            .map(|row| row.ascii_art())
            .collect();
        let expected: Vec<String> = vec![
            "  .   0   1   2   3   4   .   .   .   .   .   .   .   .   5   6   7   8   9   ."
                .into(),
            " 10  11  12  13  14  15   .   .   .   .   .   .  16  17  18  19  20  21".into(),
            " 22  23  24  25  26  27   .   .   .   .   .   .  28  29  30  31  32  33".into(),
            " 34  35  36  37  38  39   .   .   .   .   .   .  40  41  42  43  44  45".into(),
            " 46  47  48  49  50  51  52  53  54  55  56  57  58  59  60  61  62  63".into(),
            " 64  65  66  67  68   .  69  70  71  72  73  74   .  75  76  77  78  79".into(),
        ];
        assert_eq!(ascii, expected, "ASCII formatting rows drifted");
    }

    #[test]
    fn key_position_header_is_exposed() {
        let doc = KeyboardProfileDoc::from_file(&test_profile_path("glove80/profile.toml"))
            .expect("profile");
        let header = doc
            .layout
            .keymap
            .key_position_header()
            .expect("header string");
        assert!(header.contains("POS_LH_T1"));
        assert!(
            doc.layout.keymap.system_behaviors_dts().is_some(),
            "system behaviors should be exposed"
        );
    }

    #[test]
    fn loads_embedded_profile() {
        // This test will work even if profiles/keyboards/ directory is deleted
        let doc = KeyboardProfileDoc::load("glove80").expect("embedded profile");
        assert_eq!(doc.keyboard, "glove80");
        assert_eq!(doc.metadata.name, "MoErgo Glove80");
        assert_eq!(doc.hardware.key_count, 80);
    }

    #[test]
    fn lists_available_profiles() {
        let profiles = KeyboardProfileDoc::list_available();
        assert!(
            !profiles.is_empty(),
            "should have at least embedded profiles"
        );
        assert!(
            profiles.contains(&"glove80".to_string()),
            "should include glove80"
        );
    }

    #[test]
    fn loads_vendor_kconfig_options() {
        let options = super::load_vendor_kconfig_options("MoErgo")
            .expect("should load MoErgo kconfig options");
        assert!(
            !options.is_empty(),
            "should have at least one kconfig option"
        );
        // Verify a known option
        let battery = options
            .get("BATTERY_REPORT_INTERVAL_SEC")
            .expect("should have BATTERY_REPORT_INTERVAL_SEC");
        let table = battery.as_table().expect("should be a table");
        assert_eq!(
            table.get("name").and_then(|v| v.as_str()),
            Some("CONFIG_ZMK_BATTERY_REPORT_INTERVAL")
        );
        assert_eq!(
            table.get("type").and_then(|v| v.as_str()),
            Some("int")
        );
    }

    #[test]
    fn vendor_kconfig_options_returns_none_for_unknown_vendor() {
        assert!(super::load_vendor_kconfig_options("nonexistent").is_none());
    }
}
