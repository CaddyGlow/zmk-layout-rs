use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
use thiserror::Error;
use toml::Value as TomlValue;

use crate::layout_engine::{LayerSelector, MetadataMap};

use super::targets::{find_overlapping_target, normalize_locator};

/// Parsed representation of a task configuration file.
#[derive(Debug, Clone)]
pub struct TaskFile {
    pub base: BaseSection,
    pub config: ConfigSection,
    pub tasks: Vec<Task>,
    pub script_dir: Option<PathBuf>,
}

impl TaskFile {
    /// Parse a task file from raw TOML text.
    pub fn from_toml_str(contents: &str) -> Result<Self, TaskConfigError> {
        let raw: RawTaskFile = toml::from_str(contents).map_err(TaskConfigError::Parse)?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawTaskFile) -> Result<Self, TaskConfigError> {
        let base = raw.base.unwrap_or_default();
        let config = raw.config.ok_or(TaskConfigError::MissingConfig)?;
        let config: ConfigSection = config.try_into()?;

        if raw.tasks.is_empty() {
            return Err(TaskConfigError::NoTasks);
        }

        let mut ids = HashSet::new();
        let mut slug_counts = HashMap::new();
        let mut target_set = HashSet::new();
        let mut target_hierarchy = Vec::new();
        let mut tasks = Vec::with_capacity(raw.tasks.len());

        for (index, mut raw_task) in raw.tasks.into_iter().enumerate() {
            let kind = raw_task.kind;
            let target = match raw_task.target.as_ref() {
                Some(value) if !value.trim().is_empty() => normalize_locator(value),
                Some(_) => {
                    return Err(invalid(
                        kind,
                        "target",
                        index,
                        "target must be a non-empty string",
                    ));
                }
                None => return Err(missing(kind, "target", index)),
            };
            if !target_set.insert(target.clone()) {
                return Err(TaskConfigError::DuplicateTarget(target.clone()));
            }
            if let Some(existing) = find_overlapping_target(&target_hierarchy, &target) {
                return Err(TaskConfigError::OverlappingTarget {
                    existing,
                    new: target.clone(),
                });
            }
            target_hierarchy.push(target.clone());

            let id = match raw_task.id.clone() {
                Some(id) => {
                    if !ids.insert(id.clone()) {
                        return Err(TaskConfigError::DuplicateId(id));
                    }
                    id
                }
                None => {
                    let auto = auto_id(raw_task.kind, &target, &mut slug_counts);
                    if !ids.insert(auto.clone()) {
                        return Err(TaskConfigError::DuplicateId(auto));
                    }
                    auto
                }
            };

            let conflict = raw_task.conflict.unwrap_or(config.default_conflict);
            let comment = raw_task.comment.clone();

            let mut expected = raw_task.expected.take();
            if let Some(text) = expected.as_ref() {
                if text.trim().is_empty() {
                    return Err(invalid(
                        raw_task.kind,
                        "expected",
                        index,
                        "expected value cannot be empty",
                    ));
                }
            }

            let action = raw_task.into_action(index)?;
            if expected.is_none() {
                expected = match &action {
                    TaskAction::Override(action) => action.from.clone(),
                    _ => None,
                };
            }
            let expected = expected.map(|value| value.trim().to_string());

            if let TaskAction::Override(action) = &action {
                if normalize_locator(&action.path) != target {
                    return Err(invalid(
                        kind,
                        "target",
                        index,
                        format!(
                            "override target `{}` must match path `{}`",
                            target, action.path
                        ),
                    ));
                }
            }

            tasks.push(Task {
                id,
                target,
                conflict,
                comment,
                expected,
                action,
            });
        }

        Ok(TaskFile {
            base: base.into(),
            config,
            tasks,
            script_dir: None,
        })
    }

    /// Attach the directory used to resolve script paths.
    pub fn set_script_dir(&mut self, dir: impl Into<PathBuf>) {
        self.script_dir = Some(dir.into());
    }
}

#[derive(Debug, Clone)]
pub struct BaseSection {
    pub template: Option<String>,
    pub version: Option<String>,
    pub metadata: MetadataMap,
}

impl Default for BaseSection {
    fn default() -> Self {
        Self {
            template: None,
            version: None,
            metadata: MetadataMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigSection {
    pub format_version: String,
    pub default_conflict: ConflictPolicy,
    pub conflict_script: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub target: String,
    pub conflict: ConflictPolicy,
    pub comment: Option<String>,
    pub expected: Option<String>,
    pub action: TaskAction,
}

#[derive(Debug, Clone)]
pub enum TaskAction {
    Override(OverrideTask),
    Combo(ComboTask),
    Layer(LayerTask),
    LayerOrder(LayerOrderTask),
    Behavior(BehaviorTask),
    Meta(MetaTask),
    Script(ScriptTask),
}

#[derive(Debug, Clone)]
pub struct OverrideTask {
    pub path: String,
    pub value: String,
    pub from: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ComboTask {
    pub name: String,
    pub key_positions: Vec<u32>,
    pub binding: String,
    pub timeout_ms: Option<u32>,
    pub layers: Vec<LayerSelector>,
    pub conditions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LayerTask {
    pub name: String,
    pub bindings: Vec<String>,
    pub metadata: MetadataMap,
}

#[derive(Debug, Clone)]
pub enum LayerOrderMovement {
    Position(usize),
    Before(String),
    After(String),
}

#[derive(Debug, Clone)]
pub struct LayerOrderTask {
    pub layer: String,
    pub movement: LayerOrderMovement,
}

#[derive(Debug, Clone)]
pub struct BehaviorTask {
    pub behavior: String,
    pub settings: MetadataMap,
}

#[derive(Debug, Clone)]
pub struct MetaTask {
    pub key: String,
    pub value: TomlValue,
}

#[derive(Debug, Clone)]
pub enum ScriptSource {
    Inline(String),
    File(String),
}

#[derive(Debug, Clone)]
pub struct ScriptTask {
    pub source: ScriptSource,
    pub args: MetadataMap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    Prompt,
    Override,
    Skip,
    Script,
}

impl Default for ConflictPolicy {
    fn default() -> Self {
        ConflictPolicy::Prompt
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    Override,
    Combo,
    Layer,
    LayerOrder,
    Behavior,
    Meta,
    Script,
}

impl TaskKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            TaskKind::Override => "override",
            TaskKind::Combo => "combo",
            TaskKind::Layer => "layer",
            TaskKind::LayerOrder => "layer-order",
            TaskKind::Behavior => "behavior",
            TaskKind::Meta => "meta",
            TaskKind::Script => "script",
        }
    }
}

#[derive(Debug, Error)]
pub enum TaskConfigError {
    #[error("failed to parse task file: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("missing config section")]
    MissingConfig,
    #[error("missing config.format_version")]
    MissingFormatVersion,
    #[error("no tasks defined")]
    NoTasks,
    #[error("duplicate task id '{0}'")]
    DuplicateId(String),
    #[error("duplicate target '{0}'")]
    DuplicateTarget(String),
    #[error("target '{new}' overlaps with '{existing}'")]
    OverlappingTarget { existing: String, new: String },
    #[error("missing field '{field}' for task #{index} ({kind:?})")]
    MissingField {
        field: &'static str,
        kind: TaskKind,
        index: usize,
    },
    #[error("invalid field '{field}' for task #{index} ({kind:?}): {message}")]
    InvalidField {
        field: &'static str,
        kind: TaskKind,
        index: usize,
        message: String,
    },
}

#[derive(Deserialize)]
struct RawTaskFile {
    #[serde(default)]
    base: Option<RawBaseSection>,
    config: Option<RawConfigSection>,
    #[serde(default)]
    tasks: Vec<RawTask>,
}

#[derive(Deserialize, Default)]
struct RawBaseSection {
    template: Option<String>,
    version: Option<String>,
    #[serde(default)]
    metadata: MetadataMap,
}

impl From<RawBaseSection> for BaseSection {
    fn from(value: RawBaseSection) -> Self {
        BaseSection {
            template: value.template,
            version: value.version,
            metadata: value.metadata,
        }
    }
}

#[derive(Deserialize)]
struct RawConfigSection {
    format_version: Option<String>,
    #[serde(default)]
    default_conflict: Option<ConflictPolicy>,
    conflict_script: Option<String>,
    comment: Option<String>,
}

impl TryFrom<RawConfigSection> for ConfigSection {
    type Error = TaskConfigError;

    fn try_from(value: RawConfigSection) -> Result<Self, Self::Error> {
        let format_version = value
            .format_version
            .ok_or(TaskConfigError::MissingFormatVersion)?;
        Ok(ConfigSection {
            format_version,
            default_conflict: value.default_conflict.unwrap_or(ConflictPolicy::Prompt),
            conflict_script: value.conflict_script,
            comment: value.comment,
        })
    }
}

#[derive(Deserialize)]
struct RawTask {
    id: Option<String>,
    #[serde(rename = "type")]
    kind: TaskKind,
    path: Option<String>,
    target: Option<String>,
    comment: Option<String>,
    conflict: Option<ConflictPolicy>,
    value: Option<TomlValue>,
    from: Option<String>,
    name: Option<String>,
    key_positions: Option<Vec<u32>>,
    binding: Option<String>,
    timeout_ms: Option<u32>,
    layers: Option<Vec<RawLayerSelector>>,
    conditions: Option<Vec<String>>,
    bindings: Option<Vec<String>>,
    metadata: Option<MetadataMap>,
    layer: Option<String>,
    position: Option<i64>,
    before: Option<String>,
    after: Option<String>,
    behavior: Option<String>,
    settings: Option<MetadataMap>,
    key: Option<String>,
    script: Option<String>,
    filename: Option<String>,
    args: Option<MetadataMap>,
    expected: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawLayerSelector {
    Index(i64),
    Name(String),
}

impl RawTask {
    fn into_action(mut self, index: usize) -> Result<TaskAction, TaskConfigError> {
        match self.kind {
            TaskKind::Override => {
                let path = take_nonempty_string(
                    &mut self.path,
                    self.kind,
                    "path",
                    index,
                    "path cannot be empty",
                )?;
                let value = self
                    .value
                    .take()
                    .ok_or_else(|| missing(self.kind, "value", index))?;
                let value = value.as_str().map(str::to_string).ok_or_else(|| {
                    invalid(self.kind, "value", index, "override value must be a string")
                })?;
                if value.trim().is_empty() {
                    return Err(invalid(
                        self.kind,
                        "value",
                        index,
                        "override value cannot be empty",
                    ));
                }
                Ok(TaskAction::Override(OverrideTask {
                    path,
                    value,
                    from: self.from,
                }))
            }
            TaskKind::Combo => {
                let name = take_nonempty_string(
                    &mut self.name,
                    self.kind,
                    "name",
                    index,
                    "combo name cannot be empty",
                )?;
                let key_positions = self
                    .key_positions
                    .take()
                    .ok_or_else(|| missing(self.kind, "key_positions", index))?;
                if key_positions.is_empty() {
                    return Err(invalid(
                        self.kind,
                        "key_positions",
                        index,
                        "combo must declare at least one key position",
                    ));
                }
                let binding = take_nonempty_string(
                    &mut self.binding,
                    self.kind,
                    "binding",
                    index,
                    "binding cannot be empty",
                )?;
                Ok(TaskAction::Combo(ComboTask {
                    name,
                    key_positions,
                    binding,
                    timeout_ms: self.timeout_ms,
                    layers: convert_layer_selectors(self.layers, self.kind, index)?,
                    conditions: self.conditions.unwrap_or_default(),
                }))
            }
            TaskKind::Layer => {
                let name = take_nonempty_string(
                    &mut self.name,
                    self.kind,
                    "name",
                    index,
                    "layer name cannot be empty",
                )?;
                let bindings = self
                    .bindings
                    .take()
                    .ok_or_else(|| missing(self.kind, "bindings", index))?;
                if bindings.is_empty() {
                    return Err(invalid(
                        self.kind,
                        "bindings",
                        index,
                        "layer must define at least one binding",
                    ));
                }
                Ok(TaskAction::Layer(LayerTask {
                    name,
                    bindings,
                    metadata: self.metadata.unwrap_or_default(),
                }))
            }
            TaskKind::LayerOrder => {
                let movement = resolve_layer_order(&self, index)?;
                let layer = take_nonempty_string(
                    &mut self.layer,
                    self.kind,
                    "layer",
                    index,
                    "layer cannot be empty",
                )?;
                Ok(TaskAction::LayerOrder(LayerOrderTask { layer, movement }))
            }
            TaskKind::Behavior => {
                let behavior = take_nonempty_string(
                    &mut self.behavior,
                    self.kind,
                    "behavior",
                    index,
                    "behavior name cannot be empty",
                )?;
                let settings = self
                    .settings
                    .take()
                    .ok_or_else(|| missing(self.kind, "settings", index))?;
                if settings.is_empty() {
                    return Err(invalid(
                        self.kind,
                        "settings",
                        index,
                        "behavior settings cannot be empty",
                    ));
                }
                Ok(TaskAction::Behavior(BehaviorTask { behavior, settings }))
            }
            TaskKind::Meta => {
                let key = take_nonempty_string(
                    &mut self.key,
                    self.kind,
                    "key",
                    index,
                    "meta key cannot be empty",
                )?;
                let value = self
                    .value
                    .ok_or_else(|| missing(self.kind, "value", index))?;
                Ok(TaskAction::Meta(MetaTask { key, value }))
            }
            TaskKind::Script => {
                let source = match (self.filename, self.script) {
                    (Some(file), None) => {
                        if file.trim().is_empty() {
                            return Err(invalid(
                                self.kind,
                                "filename",
                                index,
                                "script filename cannot be empty",
                            ));
                        }
                        ScriptSource::File(file)
                    }
                    (None, Some(inline)) => {
                        if inline.trim().is_empty() {
                            return Err(invalid(
                                self.kind,
                                "script",
                                index,
                                "inline script cannot be empty",
                            ));
                        }
                        ScriptSource::Inline(inline)
                    }
                    (Some(file), Some(inline)) => {
                        if file.trim().is_empty() {
                            if inline.trim().is_empty() {
                                return Err(invalid(
                                    self.kind,
                                    "filename|script",
                                    index,
                                    "script task must specify a filename or inline script",
                                ));
                            }
                            ScriptSource::Inline(inline)
                        } else {
                            ScriptSource::File(file)
                        }
                    }
                    (None, None) => {
                        return Err(missing(self.kind, "filename|script", index));
                    }
                };
                Ok(TaskAction::Script(ScriptTask {
                    source,
                    args: self.args.unwrap_or_default(),
                }))
            }
        }
    }
}

fn convert_layer_selectors(
    raw: Option<Vec<RawLayerSelector>>,
    kind: TaskKind,
    index: usize,
) -> Result<Vec<LayerSelector>, TaskConfigError> {
    let Some(values) = raw else {
        return Ok(Vec::new());
    };
    let mut selectors = Vec::with_capacity(values.len());
    for value in values {
        selectors.push(value.into_selector(kind, index)?);
    }
    Ok(selectors)
}

impl RawLayerSelector {
    fn into_selector(self, kind: TaskKind, index: usize) -> Result<LayerSelector, TaskConfigError> {
        match self {
            RawLayerSelector::Index(value) => {
                if value < 0 {
                    Err(invalid(
                        kind,
                        "layers",
                        index,
                        "layer index must be non-negative",
                    ))
                } else {
                    Ok(LayerSelector::Index(value as u32))
                }
            }
            RawLayerSelector::Name(name) => {
                if name.trim().is_empty() {
                    Err(invalid(kind, "layers", index, "layer name cannot be empty"))
                } else {
                    Ok(LayerSelector::Name(name.trim().to_string()))
                }
            }
        }
    }
}

fn take_nonempty_string(
    slot: &mut Option<String>,
    kind: TaskKind,
    field: &'static str,
    index: usize,
    empty_message: &'static str,
) -> Result<String, TaskConfigError> {
    match slot.take() {
        Some(value) if !value.trim().is_empty() => Ok(value),
        Some(_) => Err(invalid(kind, field, index, empty_message)),
        None => Err(missing(kind, field, index)),
    }
}

fn resolve_layer_order(
    task: &RawTask,
    index: usize,
) -> Result<LayerOrderMovement, TaskConfigError> {
    let has_position = task.position.is_some();
    let has_before = task.before.is_some();
    let has_after = task.after.is_some();
    let count = has_position as u8 + has_before as u8 + has_after as u8;

    if count == 0 {
        return Err(missing(task.kind, "position|before|after", index));
    }
    if count > 1 {
        return Err(invalid(
            task.kind,
            "position|before|after",
            index,
            "specify only one of position/before/after",
        ));
    }

    if let Some(pos) = task.position {
        if pos < 0 {
            return Err(invalid(
                task.kind,
                "position",
                index,
                "position must be positive",
            ));
        }
        return Ok(LayerOrderMovement::Position(pos as usize));
    }
    if let Some(name) = &task.before {
        if name.trim().is_empty() {
            return Err(invalid(
                task.kind,
                "before",
                index,
                "layer reference cannot be empty",
            ));
        }
        return Ok(LayerOrderMovement::Before(name.clone()));
    }
    if let Some(name) = &task.after {
        if name.trim().is_empty() {
            return Err(invalid(
                task.kind,
                "after",
                index,
                "layer reference cannot be empty",
            ));
        }
        return Ok(LayerOrderMovement::After(name.clone()));
    }
    unreachable!("validated combination should ensure one branch returns")
}

fn missing(kind: TaskKind, field: &'static str, index: usize) -> TaskConfigError {
    TaskConfigError::MissingField { field, kind, index }
}

fn invalid(
    kind: TaskKind,
    field: &'static str,
    index: usize,
    message: impl Into<String>,
) -> TaskConfigError {
    TaskConfigError::InvalidField {
        field,
        kind,
        index,
        message: message.into(),
    }
}

fn auto_id(kind: TaskKind, target: &str, slug_counts: &mut HashMap<String, usize>) -> String {
    let base = slugify(&format!("{}-{}", kind.as_str(), target));
    let entry = slug_counts.entry(base.clone()).or_insert(0);
    *entry += 1;
    if *entry == 1 {
        base
    } else {
        format!("{}-{}", base, entry)
    }
}

fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str("task");
    }
    slug
}
