use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::Path,
};

use serde_json::Value;

use crate::adapters::{
    bundle::{
        BundleError, BundleFirmware, BundleMetadata, BundleOutput, BundleOverlays, BundleSource,
        BundleSymbols, BundleTarget, LayoutBundle,
    },
    moergo::{
        behaviors::{BehaviorMetadata, load_behavior_metadata},
        mapping::{MoergoKconfigMap, load_kconfig_map},
        spec::{
            MoergoBinding, MoergoCombo, MoergoHoldTap, MoergoInputListener,
            MoergoInputListenerNode, MoergoInputProcessor, MoergoLayout, MoergoMacro,
        },
        template::{DEFAULT_TARGET_ID, DEFAULT_TEMPLATE_PATH, SOURCE_NAME},
    },
    standard::{
        AdapterLayout, BehaviorSpec, ComboSpec, InputListenerNodeSpec, InputListenerSpec,
        LayoutMetadata, MacroSpec,
    },
};
use crate::profiles::KeyboardProfileDoc;

/// Build a bundle from a MoErgo JSON payload.
pub fn import_bundle_from_str(json: &str) -> Result<LayoutBundle, BundleError> {
    let payload: MoergoLayout = serde_json::from_str(json)?;
    let profile = payload
        .keyboard
        .as_deref()
        .and_then(|name| KeyboardProfileDoc::load(name).ok());
    let mapping = load_kconfig_map()?;

    let layers = build_layers(&payload);
    let combos = payload
        .combos
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(moergo_combo_to_combo_spec)
        .collect();
    let macros = payload
        .macros
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(moergo_macro_to_macro_spec)
        .collect();
    let behaviors = payload
        .hold_taps
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(moergo_hold_tap_to_behavior_spec)
        .collect();
    let input_listeners = payload
        .input_listeners
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(moergo_input_listener_to_spec)
        .collect();

    let mut layout = AdapterLayout {
        layers,
        combos,
        behaviors,
        macros,
        input_listeners,
        metadata: LayoutMetadata {
            title: payload.title.clone(),
            description: payload.notes.clone(),
            author: payload.creator.clone(),
            version: payload.firmware_api_version.as_ref().map(|v| v.to_string()),
            extras: BTreeMap::new(),
        },
    };
    layout.ensure_property_orders();

    let mut metadata = BundleMetadata::default();
    metadata.title = payload.title.clone();
    metadata.description = payload.notes.clone();
    metadata.keyboard = payload.keyboard.clone();
    metadata.tags = payload.tags.clone().unwrap_or_default();
    if let Some(profile) = profile.as_ref() {
        if let Some(system) = profile.layout.keymap.system_behaviors_dts() {
            metadata.extras.insert(
                "system_behaviors_dts".into(),
                Value::String(system.to_string()),
            );
        }
    }

    let overlays = build_overlays(
        &payload,
        profile.as_ref().and_then(|doc| {
            doc.layout
                .keymap
                .key_position_header()
                .map(|s| s.to_string())
        }),
    );
    let mut config = config_parameters_to_defines(&payload.config_parameters, &mapping);
    let behavior_metadata = load_behavior_metadata()?;

    let mut symbols = BundleSymbols::default();
    if let Some(locale) = payload.locale.clone() {
        symbols
            .template_vars
            .insert("locale".into(), Value::String(locale));
    }
    let profile_includes: Vec<String> = profile
        .as_ref()
        .map(|doc| doc.layout.keymap.header_includes.clone())
        .unwrap_or_default();
    let (includes, required_configs) =
        collect_behavior_includes_and_configs(&layout, &behavior_metadata, &profile_includes);
    symbols.includes = includes;

    for name in required_configs {
        push_required_define(&mut config, &name);
    }

    symbols.defines = config.defines.clone();

    metadata.extras.insert(
        "moergo".into(),
        moergo_metadata_block(&payload, &config.alias_order),
    );

    let target = BundleTarget {
        id: DEFAULT_TARGET_ID.to_string(),
        kind: Some("moergo".to_string()),
        firmware: Some(BundleFirmware {
            repo: Some("moergo-sc/zmk".to_string()),
            channel: Some("stable".to_string()),
            version: None,
            board: payload.keyboard.clone(),
        }),
        template: Some(DEFAULT_TEMPLATE_PATH.to_string()),
        overlays: overlays.default_overlay_names(),
        defines: ensure_define_order(&config.define_order, &symbols.defines),
        includes: vec![],
        output: Some(BundleOutput {
            format: "dtsi".to_string(),
        }),
    };

    let mut sources = BTreeMap::new();
    sources.insert(
        SOURCE_NAME.into(),
        BundleSource {
            path: None,
            schema_version: payload.firmware_api_version.clone(),
            fingerprint: None,
            notes: None,
        },
    );

    let mut bundle = LayoutBundle::default();
    bundle.metadata = metadata;
    bundle.layout = layout;
    bundle.overlays = overlays;
    bundle.symbols = symbols;
    bundle.targets = vec![target];
    bundle.sources = sources;
    bundle.validate()?;
    Ok(bundle)
}

/// Build a bundle from a MoErgo JSON file.
pub fn import_bundle_from_file(path: impl AsRef<Path>) -> Result<LayoutBundle, BundleError> {
    let json = fs::read_to_string(path)?;
    import_bundle_from_str(&json)
}

/// Serialize a bundle back into a MoErgo JSON payload.
pub fn export_bundle_to_moergo_json(bundle: &LayoutBundle) -> Result<String, BundleError> {
    let mapping = load_kconfig_map()?;
    let layout = &bundle.layout;
    let layers = layout
        .layers
        .iter()
        .map(|layer| {
            layer
                .bindings
                .iter()
                .map(|binding| string_binding_to_moergo_binding(binding.as_str()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let combos = layout
        .combos
        .iter()
        .map(combo_spec_to_moergo_combo)
        .collect();
    let macros = layout
        .macros
        .iter()
        .map(macro_spec_to_moergo_macro)
        .collect();
    let hold_taps = extract_hold_taps(layout);

    let input_listeners = layout
        .input_listeners
        .iter()
        .map(spec_to_moergo_input_listener)
        .collect();

    let preferred_alias_order =
        extract_config_alias_order(&bundle.metadata, &bundle.symbols, &bundle.targets);
    let config_parameters =
        defines_to_config_parameters(&bundle.symbols.defines, &mapping, &preferred_alias_order);

    let moergo = MoergoLayout {
        keyboard: bundle.metadata.keyboard.clone(),
        firmware_api_version: bundle
            .sources
            .get(SOURCE_NAME)
            .and_then(|src| src.schema_version.clone()),
        locale: bundle
            .symbols
            .template_vars
            .get("locale")
            .and_then(|val| val.as_str().map(|s| s.to_string())),
        uuid: None,
        parent_uuid: None,
        unlisted: None,
        date: None,
        creator: layout.metadata.author.clone(),
        title: layout.metadata.title.clone(),
        notes: layout.metadata.description.clone(),
        tags: Some(bundle.metadata.tags.clone()),
        custom_defined_behaviors: bundle.overlays.custom_behaviors.clone().unwrap_or_default(),
        custom_devicetree: bundle
            .overlays
            .custom_devicetree
            .clone()
            .unwrap_or_default(),
        config_parameters,
        layout_parameters: None,
        combos: Some(combos),
        layer_names: layout
            .layers
            .iter()
            .map(|layer| layer.name.clone())
            .collect(),
        layers,
        macros: Some(macros),
        hold_taps: if hold_taps.is_empty() {
            None
        } else {
            Some(hold_taps)
        },
        input_listeners: Some(input_listeners),
        key_position_header: bundle
            .overlays
            .fragments
            .get("key_position_header")
            .cloned(),
    };
    Ok(serde_json::to_string_pretty(&moergo)?)
}

fn build_layers(payload: &MoergoLayout) -> Vec<crate::adapters::standard::LayerSpec> {
    let mut layers = Vec::new();
    for (idx, name) in payload.layer_names.iter().enumerate() {
        let bindings = payload
            .layers
            .get(idx)
            .map(|row| row.iter().map(binding_to_string).collect())
            .unwrap_or_default();
        layers.push(crate::adapters::standard::LayerSpec {
            name: name.clone(),
            bindings,
        });
    }
    layers
}

fn binding_to_string(binding: &MoergoBinding) -> String {
    let mut parts = Vec::new();
    parts.push(binding_value_to_string(&binding.value));
    for param in &binding.params {
        parts.push(binding_to_string(param));
    }
    parts.join(" ")
}

fn binding_value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(num) => num.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn moergo_combo_to_combo_spec(combo: MoergoCombo) -> ComboSpec {
    ComboSpec {
        name: combo.name,
        description: combo.description.unwrap_or_default(),
        key_positions: combo.key_positions,
        timeout_ms: combo.timeout_ms,
        layers: combo
            .layers
            .into_iter()
            .map(|layer| layer.max(0) as u32)
            .collect(),
        binding: Some(binding_to_string(&combo.binding)),
        behavior: None,
        properties: BTreeMap::new(),
        property_order: Vec::new(),
    }
}

fn moergo_macro_to_macro_spec(m: MoergoMacro) -> MacroSpec {
    MacroSpec {
        name: m.name,
        description: m.description.unwrap_or_default(),
        wait_ms: m.wait_ms,
        tap_ms: m.tap_ms,
        bindings: m
            .bindings
            .into_iter()
            .map(|binding| binding_to_string(&binding))
            .collect(),
        binding_cells: None,
        compatible: None,
        label: None,
        properties: BTreeMap::new(),
        property_order: Vec::new(),
    }
}

fn moergo_hold_tap_to_behavior_spec(ht: MoergoHoldTap) -> BehaviorSpec {
    let mut properties = BTreeMap::new();
    if let Some(term) = ht.tapping_term_ms {
        properties.insert("tapping-term-ms".into(), term.to_string());
    }
    if let Some(flavor) = ht.flavor {
        properties.insert("flavor".into(), flavor);
    }
    if let Some(qt) = ht.quick_tap_ms {
        properties.insert("quick-tap-ms".into(), qt.to_string());
    }
    if let Some(req) = ht.require_prior_idle_ms {
        properties.insert("require-prior-idle-ms".into(), req.to_string());
    }
    if let Some(pos) = ht.hold_trigger_key_positions {
        let rendered = pos
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        properties.insert("hold-trigger-key-positions".into(), rendered);
    }
    if let Some(on_release) = ht.hold_trigger_on_release {
        properties.insert("hold-trigger-on-release".into(), on_release.to_string());
    }
    BehaviorSpec {
        name: ht.name,
        description: ht.description.unwrap_or_default(),
        compatible: Some("zmk,behavior-hold-tap".into()),
        binding_cells: None,
        label: None,
        bindings: ht.bindings,
        properties,
        property_order: Vec::new(),
    }
}

fn moergo_input_listener_to_spec(listener: MoergoInputListener) -> InputListenerSpec {
    InputListenerSpec {
        code: listener.code,
        input_processors: listener
            .input_processors
            .into_iter()
            .map(|proc| crate::adapters::standard::InputProcessorSpec {
                code: proc.code,
                params: proc.params,
            })
            .collect(),
        nodes: listener
            .nodes
            .into_iter()
            .map(|node| InputListenerNodeSpec {
                code: node.code,
                description: node.description,
                layers: node.layers,
                input_processors: node
                    .input_processors
                    .into_iter()
                    .map(|proc| crate::adapters::standard::InputProcessorSpec {
                        code: proc.code,
                        params: proc.params,
                    })
                    .collect(),
                properties: BTreeMap::new(),
                property_order: Vec::new(),
            })
            .collect(),
        properties: BTreeMap::new(),
        property_order: Vec::new(),
    }
}

fn string_binding_to_moergo_binding(binding: &str) -> MoergoBinding {
    let mut tokens = binding
        .split_whitespace()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty());
    let value = tokens.next().unwrap_or_default();
    let params = tokens
        .map(|param| MoergoBinding {
            value: Value::String(param),
            params: Vec::new(),
        })
        .collect();
    MoergoBinding {
        value: Value::String(value),
        params,
    }
}

fn combo_spec_to_moergo_combo(combo: &ComboSpec) -> MoergoCombo {
    MoergoCombo {
        name: combo.name.clone(),
        description: Some(combo.description.clone()),
        binding: combo
            .binding
            .as_ref()
            .map(|binding| string_binding_to_moergo_binding(binding))
            .unwrap_or_else(|| MoergoBinding {
                value: Value::String(String::new()),
                params: Vec::new(),
            }),
        key_positions: combo.key_positions.clone(),
        timeout_ms: combo.timeout_ms,
        layers: combo.layers.iter().map(|layer| *layer as i32).collect(),
    }
}

fn macro_spec_to_moergo_macro(mac: &MacroSpec) -> MoergoMacro {
    MoergoMacro {
        name: mac.name.clone(),
        description: Some(mac.description.clone()),
        bindings: mac
            .bindings
            .iter()
            .map(|binding| string_binding_to_moergo_binding(binding))
            .collect(),
        params: Vec::new(),
        wait_ms: mac.wait_ms,
        tap_ms: mac.tap_ms,
    }
}

fn extract_hold_taps(layout: &AdapterLayout) -> Vec<MoergoHoldTap> {
    layout
        .behaviors
        .iter()
        .filter(|behavior| {
            behavior
                .compatible
                .as_deref()
                .map(|compat: &str| compat.contains("hold-tap"))
                .unwrap_or(false)
        })
        .map(|behavior| {
            let mut ht = MoergoHoldTap {
                name: behavior.name.clone(),
                description: Some(behavior.description.clone()),
                bindings: behavior.bindings.clone(),
                tapping_term_ms: behavior
                    .property_value("tapping-term-ms")
                    .and_then(|v: &str| v.parse::<u32>().ok()),
                flavor: behavior
                    .property_value("flavor")
                    .map(|s: &str| s.to_string()),
                quick_tap_ms: behavior
                    .property_value("quick-tap-ms")
                    .and_then(|v: &str| v.parse::<u32>().ok()),
                require_prior_idle_ms: behavior
                    .property_value("require-prior-idle-ms")
                    .and_then(|v: &str| v.parse::<u32>().ok()),
                hold_trigger_key_positions: behavior
                    .property_value("hold-trigger-key-positions")
                    .map(|v| parse_numbers(v)),
                hold_trigger_on_release: behavior
                    .property_value("hold-trigger-on-release")
                    .map(|v: &str| v.eq_ignore_ascii_case("true")),
            };
            if ht.description.as_ref().is_some_and(|d| d.is_empty()) {
                ht.description = None;
            }
            ht
        })
        .collect()
}

fn parse_numbers(raw: &str) -> Vec<u32> {
    raw.split_whitespace()
        .filter_map(|token| token.parse::<u32>().ok())
        .collect()
}

fn spec_to_moergo_input_listener(spec: &InputListenerSpec) -> MoergoInputListener {
    MoergoInputListener {
        code: spec.code.clone(),
        input_processors: spec
            .input_processors
            .iter()
            .map(|proc| MoergoInputProcessor {
                code: proc.code.clone(),
                params: proc.params.clone(),
            })
            .collect(),
        nodes: spec
            .nodes
            .iter()
            .map(|node| MoergoInputListenerNode {
                code: node.code.clone(),
                description: node.description.clone(),
                layers: node.layers.clone(),
                input_processors: node
                    .input_processors
                    .iter()
                    .map(|proc| MoergoInputProcessor {
                        code: proc.code.clone(),
                        params: proc.params.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn moergo_metadata_block(payload: &MoergoLayout, config_order: &[String]) -> Value {
    let mut map = serde_json::Map::new();
    if let Some(locale) = payload.locale.as_ref() {
        map.insert("locale".into(), Value::String(locale.clone()));
    }
    if let Some(uuid) = payload.uuid.as_ref() {
        map.insert("uuid".into(), Value::String(uuid.clone()));
    }
    if let Some(parent) = payload.parent_uuid.as_ref() {
        map.insert("parent_uuid".into(), Value::String(parent.clone()));
    }
    if let Some(unlisted) = payload.unlisted {
        map.insert("unlisted".into(), Value::Bool(unlisted));
    }
    if let Some(date) = payload.date {
        map.insert("date".into(), Value::Number(date.into()));
    }
    if let Some(params) = payload.config_parameters.as_ref() {
        map.insert("config_parameters".into(), params.clone());
    }
    if !config_order.is_empty() {
        map.insert(
            "config_parameter_order".into(),
            Value::Array(
                config_order
                    .iter()
                    .map(|v| Value::String(v.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(params) = payload.layout_parameters.as_ref() {
        map.insert("layout_parameters".into(), params.clone());
    }
    Value::Object(map)
}

fn build_overlays(payload: &MoergoLayout, fallback_header: Option<String>) -> BundleOverlays {
    BundleOverlays {
        custom_devicetree: non_empty(payload.custom_devicetree.clone()),
        custom_behaviors: non_empty(payload.custom_defined_behaviors.clone()),
        custom_macros: None,
        input_listeners: None,
        fragments: {
            let mut map = BTreeMap::new();
            if let Some(header) = payload
                .key_position_header
                .clone()
                .or_else(|| fallback_header)
            {
                if !header.trim().is_empty() {
                    map.insert("key_position_header".to_string(), header);
                }
            }
            map
        },
    }
}

#[derive(Debug, Default)]
struct ConfigParameters {
    defines: BTreeMap<String, Value>,
    define_order: Vec<String>,
    alias_order: Vec<String>,
}

fn config_parameters_to_defines(
    params: &Option<Value>,
    mapping: &MoergoKconfigMap,
) -> ConfigParameters {
    let mut parsed = ConfigParameters::default();
    let Some(params) = params else {
        return parsed;
    };
    match params {
        Value::Array(entries) => {
            for entry in entries {
                if let Some((alias, value)) = parse_config_param_entry(entry) {
                    push_define(&mut parsed, &alias, value, mapping);
                }
            }
        }
        Value::Object(map) => {
            for (alias, value) in map {
                let trimmed = alias.trim();
                if trimmed.is_empty() {
                    continue;
                }
                push_define(&mut parsed, trimmed, value.clone(), mapping);
            }
        }
        Value::String(alias) => {
            let trimmed = alias.trim();
            if !trimmed.is_empty() {
                push_define(&mut parsed, trimmed, Value::Bool(true), mapping);
            }
        }
        _ => {}
    }
    parsed
}

fn parse_config_param_entry(entry: &Value) -> Option<(String, Value)> {
    match entry {
        Value::Object(obj) => {
            let name = obj
                .get("paramName")
                .or_else(|| obj.get("name"))
                .or_else(|| obj.get("key"))
                .and_then(|val| val.as_str())
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())?;
            let value = obj
                .get("value")
                .cloned()
                .unwrap_or_else(|| Value::Bool(true));
            Some((name, value))
        }
        Value::String(name) => {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some((trimmed.to_string(), Value::Bool(true)))
            }
        }
        _ => None,
    }
}

fn push_define(
    parsed: &mut ConfigParameters,
    alias: &str,
    value: Value,
    mapping: &MoergoKconfigMap,
) {
    if alias.trim().is_empty() {
        return;
    }
    let canonical = mapping
        .canonical_for_alias(alias)
        .map(|name| name.to_string())
        .unwrap_or_else(|| {
            let normalized = normalize_canonical(alias);
            log::warn!(
                "unknown MoErgo config alias `{alias}`, storing as `{normalized}` for round-tripping"
            );
            normalized
        });
    if parsed.defines.contains_key(&canonical) {
        return;
    }
    parsed.defines.insert(canonical.clone(), value);
    parsed.define_order.push(canonical.clone());
    parsed.alias_order.push(alias.to_string());
}

fn normalize_canonical(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("CONFIG_ZMK_") || trimmed.starts_with("CONFIG_") {
        trimmed.to_string()
    } else {
        format!("CONFIG_ZMK_{trimmed}")
    }
}

fn push_required_define(config: &mut ConfigParameters, name: &str) {
    let canonical = normalize_canonical(name);
    if config.defines.contains_key(&canonical) {
        return;
    }
    config.defines.insert(canonical.clone(), Value::Bool(true));
    config.define_order.push(canonical);
}

fn collect_behavior_includes_and_configs(
    layout: &AdapterLayout,
    metadata: &BehaviorMetadata,
    profile_includes: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut includes = Vec::new();
    let mut required_configs = Vec::new();
    let mut include_seen = HashSet::new();
    let mut config_seen = HashSet::new();

    // Always include the behaviors base header.
    include_if_new(
        &mut includes,
        &mut include_seen,
        "#include <behaviors.dtsi>",
    );

    for inc in profile_includes {
        include_if_new(&mut includes, &mut include_seen, inc);
    }

    let codes = collect_behavior_codes(layout);
    for code in codes {
        if let Some(extra_includes) = metadata.includes_for(&code) {
            for inc in extra_includes {
                include_if_new(&mut includes, &mut include_seen, inc);
            }
        }
        if let Some(configs) = metadata.required_configs_for(&code) {
            for cfg in configs {
                if config_seen.insert(cfg.to_string()) {
                    required_configs.push(cfg.to_string());
                }
            }
        }
    }

    // Input listeners pull in processor includes.
    if !layout.input_listeners.is_empty() {
        include_if_new(
            &mut includes,
            &mut include_seen,
            "#include <input/processors.dtsi>",
        );
    }

    (includes, required_configs)
}

fn collect_behavior_codes(layout: &AdapterLayout) -> BTreeSet<String> {
    let mut codes = BTreeSet::new();
    for layer in &layout.layers {
        for binding in &layer.bindings {
            if let Some(code) = first_binding_token(binding) {
                codes.insert(code);
            }
        }
    }
    for combo in &layout.combos {
        if let Some(binding) = combo.binding.as_deref() {
            if let Some(code) = first_binding_token(binding) {
                codes.insert(code);
            }
        }
    }
    for mac in &layout.macros {
        for binding in &mac.bindings {
            if let Some(code) = first_binding_token(binding) {
                codes.insert(code);
            }
        }
    }
    for behavior in &layout.behaviors {
        for binding in &behavior.bindings {
            if let Some(code) = first_binding_token(binding) {
                codes.insert(code);
            }
        }
    }
    codes
}

fn first_binding_token(binding: &str) -> Option<String> {
    binding
        .split_whitespace()
        .find(|token| token.starts_with('&'))
        .map(|token| token.trim().to_string())
}

fn include_if_new(includes: &mut Vec<String>, seen: &mut HashSet<String>, inc: &str) {
    let trimmed = inc.trim();
    if trimmed.is_empty() {
        return;
    }
    if seen.insert(trimmed.to_string()) {
        includes.push(trimmed.to_string());
    }
}

fn ensure_define_order(preferred: &[String], defines: &BTreeMap<String, Value>) -> Vec<String> {
    if preferred.is_empty() {
        return defines.keys().cloned().collect();
    }
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    for name in preferred {
        if defines.contains_key(name) && seen.insert(name.clone()) {
            order.push(name.clone());
        }
    }
    for name in defines.keys() {
        if seen.insert(name.clone()) {
            order.push(name.clone());
        }
    }
    order
}

fn defines_to_config_parameters(
    defines: &BTreeMap<String, Value>,
    mapping: &MoergoKconfigMap,
    preferred_alias_order: &[String],
) -> Option<Value> {
    if defines.is_empty() {
        return None;
    }
    let mut pairs = Vec::new();
    let mut seen_canonicals = HashSet::new();

    for alias in preferred_alias_order {
        if let Some(canonical) = mapping.canonical_for_alias(alias) {
            if defines.contains_key(canonical) && seen_canonicals.insert(canonical.to_string()) {
                pairs.push((alias.to_string(), canonical.to_string(), true));
            }
        }
    }

    for (canonical, _) in defines {
        if !seen_canonicals.insert(canonical.clone()) {
            continue;
        }
        let (alias, known) = alias_for_canonical(canonical, mapping);
        if !known {
            log::warn!("no MoErgo alias found for `{canonical}`, exporting as `{alias}` instead");
        }
        pairs.push((alias, canonical.clone(), known));
    }

    if pairs.is_empty() {
        return None;
    }

    let mut params = Vec::new();
    for (alias, canonical, _) in pairs {
        if let Some(value) = defines.get(&canonical) {
            let mut obj = serde_json::Map::new();
            obj.insert("paramName".into(), Value::String(alias));
            obj.insert("value".into(), value.clone());
            params.push(Value::Object(obj));
        }
    }
    Some(Value::Array(params))
}

fn alias_for_canonical(canonical: &str, mapping: &MoergoKconfigMap) -> (String, bool) {
    if let Some(alias) = mapping.alias_for_canonical(canonical) {
        return (alias.to_string(), true);
    }
    if canonical.starts_with("CONFIG_") {
        (canonical.to_string(), false)
    } else {
        (normalize_canonical(canonical), false)
    }
}

fn extract_config_alias_order(
    metadata: &BundleMetadata,
    symbols: &BundleSymbols,
    targets: &[BundleTarget],
) -> Vec<String> {
    let Some(moergo_meta) = metadata
        .extras
        .get("moergo")
        .and_then(|val| val.as_object())
    else {
        return guess_alias_order_from_targets(symbols, targets);
    };

    if let Some(order) = moergo_meta
        .get("config_parameter_order")
        .and_then(|val| val.as_array())
    {
        let mut aliases = Vec::new();
        for val in order {
            if let Some(alias) = val.as_str() {
                aliases.push(alias.to_string());
            }
        }
        if !aliases.is_empty() {
            return aliases;
        }
    }

    if let Some(params) = moergo_meta.get("config_parameters") {
        if let Some(aliases) = extract_aliases_from_config_value(params) {
            if !aliases.is_empty() {
                return aliases;
            }
        }
    }

    guess_alias_order_from_targets(symbols, targets)
}

fn extract_aliases_from_config_value(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Array(entries) => {
            let mut order = Vec::new();
            for entry in entries {
                if let Some((alias, _)) = parse_config_param_entry(entry) {
                    order.push(alias);
                }
            }
            Some(order)
        }
        Value::Object(map) => {
            let mut order = Vec::new();
            for alias in map.keys() {
                let trimmed = alias.trim();
                if !trimmed.is_empty() {
                    order.push(trimmed.to_string());
                }
            }
            Some(order)
        }
        Value::String(alias) => {
            let trimmed = alias.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(vec![trimmed.to_string()])
            }
        }
        _ => None,
    }
}

fn guess_alias_order_from_targets(
    symbols: &BundleSymbols,
    targets: &[BundleTarget],
) -> Vec<String> {
    let mut canonical_order = Vec::new();
    for target in targets {
        for define in &target.defines {
            canonical_order.push(define.clone());
        }
    }
    if canonical_order.is_empty() {
        canonical_order.extend(symbols.defines.keys().cloned());
    }
    canonical_order
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}
