use std::{fs, rc::Rc};

use mlua::{
    Lua, Result as LuaResult, Table as LuaTable, UserData, UserDataMethods, Value as LuaValue,
};

use super::{
    behavior::BehaviorObject,
    combo::ComboObject,
    conditional::ConditionalObject,
    input::InputObject,
    layer::LayerBuilder,
    macro_builder::MacroObject,
    query::{
        BehaviorInfoObject, ComboInfoObject, LayerInfoObject, list_behavior_definitions,
        list_combo_definitions,
    },
    util::{SharedLayout, SharedLogs, require_positive_index, script_error},
};

use crate::{
    adapters::{
        pipeline::AdapterPipeline,
        standard::{
            AdapterLayout, import_standard_file_with_template, import_standard_str_with_template,
            render_standard_template,
        },
    },
    build::{
        BuildReport, BuildRequest, CliDockerBackend, CliProgressReporter, FirmwareBuilder,
        NoopProgressReporter,
    },
    dts::DtsDocument,
    io::serialize_keymap,
    keymap::KeymapDocument,
    layout_engine::LayoutEngine,
};
use serde_json;
use toml::{Value as TomlValue, map::Map as TomlMap};

#[derive(Clone)]
pub struct LayoutApi {
    layout: SharedLayout,
    #[allow(dead_code)]
    logs: SharedLogs,
}

impl LayoutApi {
    pub fn new(layout: SharedLayout, logs: SharedLogs) -> Self {
        Self { layout, logs }
    }
}

impl UserData for LayoutApi {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("layer", |_, this, name: String| {
            Ok(LayerBuilder::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("combo", |_, this, name: String| {
            Ok(ComboObject::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("behavior", |_, this, name: String| {
            Ok(BehaviorObject::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("macro", |_, this, name: String| {
            Ok(MacroObject::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("input", |_, this, name: String| {
            Ok(InputObject::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("conditional", |_, this, name: String| {
            Ok(ConditionalObject::new(name, Rc::clone(&this.layout)))
        });

        methods.add_method("new", |_, this, ()| {
            *this.layout.borrow_mut() = LayoutEngine::empty();
            Ok(())
        });

        methods.add_method("move_layer", |_, this, (name, index): (String, i64)| {
            let normalized = require_positive_index(index, "layer")?;
            this.layout
                .borrow_mut()
                .reorder_layer(&name, normalized)
                .map_err(|err| script_error(err.to_string()))
        });

        methods.add_method("remove_layer", |_, this, name: String| {
            this.layout
                .borrow_mut()
                .remove_layer(&name)
                .map_err(|err| script_error(err.to_string()))
        });

        methods.add_method("meta", |_, this, (key, value): (String, LuaValue)| {
            let toml = lua_value_to_toml(value)?;
            this.layout
                .borrow_mut()
                .set_meta_entry(&key, &toml)
                .map_err(|err| script_error(err.to_string()))
        });

        methods.add_method("get_layer", |_, this, name: String| {
            let engine = this.layout.borrow();
            match LayerInfoObject::from_engine(&engine, &name)? {
                Some(info) => Ok(Some(info)),
                None => Ok(None),
            }
        });

        methods.add_method("get_combo", |_, this, name: String| {
            let engine = this.layout.borrow();
            let def = list_combo_definitions(&engine)
                .into_iter()
                .find(|combo| combo.name == name);
            Ok(def.map(ComboInfoObject::from_definition))
        });

        methods.add_method("get_behavior", |_, this, name: String| {
            let engine = this.layout.borrow();
            let def = list_behavior_definitions(&engine)
                .into_iter()
                .find(|behavior| behavior.name == name);
            Ok(def.map(BehaviorInfoObject::from_definition))
        });

        methods.add_method("list_layers", |lua, this, ()| {
            let engine = this.layout.borrow();
            let names = engine.layer_names();
            lua.create_sequence_from(names)
        });

        methods.add_method("list_combos", |lua, this, ()| {
            let engine = this.layout.borrow();
            let names = list_combo_definitions(&engine)
                .into_iter()
                .map(|combo| combo.name)
                .collect::<Vec<_>>();
            lua.create_sequence_from(names)
        });

        methods.add_method("list_behaviors", |lua, this, ()| {
            let engine = this.layout.borrow();
            let names = list_behavior_definitions(&engine)
                .into_iter()
                .map(|behavior| behavior.name)
                .collect::<Vec<_>>();
            lua.create_sequence_from(names)
        });

        methods.add_method("load_dtsi", |_, this, path: String| {
            let text = fs::read_to_string(&path)
                .map_err(|err| script_error(format!("failed to read {path}: {err}")))?;
            let doc = DtsDocument::parse_str(&text)
                .map_err(|err| script_error(format!("failed to parse {path}: {err}")))?;
            let keymap = KeymapDocument::from_document(doc);
            *this.layout.borrow_mut() = LayoutEngine::new(keymap);
            Ok(())
        });
        methods.add_method("load_dts", |_, this, path: String| {
            let text = fs::read_to_string(&path)
                .map_err(|err| script_error(format!("failed to read {path}: {err}")))?;
            let doc = DtsDocument::parse_str(&text)
                .map_err(|err| script_error(format!("failed to parse {path}: {err}")))?;
            let keymap = KeymapDocument::from_document(doc);
            *this.layout.borrow_mut() = LayoutEngine::new(keymap);
            Ok(())
        });

        methods.add_method(
            "load_json",
            |_, this, (json_path, template_path): (String, String)| {
                let doc = import_standard_file_with_template(&json_path, &template_path)
                    .map_err(|err| script_error(format!("failed to import {json_path}: {err}")))?;
                let keymap = KeymapDocument::from_document(doc);
                *this.layout.borrow_mut() = LayoutEngine::new(keymap);
                Ok(())
            },
        );

        methods.add_method("save_dtsi", |_, this, path: String| {
            let keymap = this.layout.borrow().document().clone();
            let rendered = serialize_keymap(keymap)
                .map_err(|err| script_error(format!("failed to render DTS: {err}")))?;
            fs::write(&path, rendered)
                .map_err(|err| script_error(format!("failed to write {path}: {err}")))
        });
        methods.add_method("save_dts", |_, this, path: String| {
            let keymap = this.layout.borrow().document().clone();
            let rendered = serialize_keymap(keymap)
                .map_err(|err| script_error(format!("failed to render DTS: {err}")))?;
            fs::write(&path, rendered)
                .map_err(|err| script_error(format!("failed to write {path}: {err}")))
        });

        methods.add_method("save_json", |_, this, path: String| {
            let document = this.layout.borrow();
            let keymap = document.document().clone();
            let adapter: AdapterLayout = keymap.into();
            let json = adapter
                .to_standard_json()
                .map_err(|err| script_error(format!("failed to export JSON: {err}")))?;
            fs::write(&path, json)
                .map_err(|err| script_error(format!("failed to write {path}: {err}")))?;
            Ok(())
        });

        methods.add_method("parse_dts", |_, this, source: String| {
            let doc = DtsDocument::parse_str(&source)
                .map_err(|err| script_error(format!("failed to parse DTS: {err}")))?;
            let keymap = KeymapDocument::from_document(doc);
            *this.layout.borrow_mut() = LayoutEngine::new(keymap);
            Ok(())
        });

        methods.add_method(
            "parse_json",
            |_, this, (json, template_path): (String, String)| {
                let template = fs::read_to_string(&template_path).map_err(|err| {
                    script_error(format!("failed to read {template_path}: {err}"))
                })?;
                let doc = import_standard_str_with_template(&json, &template)
                    .map_err(|err| script_error(format!("failed to parse JSON: {err}")))?;
                *this.layout.borrow_mut() = LayoutEngine::new(KeymapDocument::from_document(doc));
                Ok(())
            },
        );

        methods.add_method("to_dts_string", |_, this, ()| {
            let keymap = this.layout.borrow().document().clone();
            serialize_keymap(keymap)
                .map_err(|err| script_error(format!("failed to serialize DTS: {err}")))
        });

        methods.add_method("to_json_string", |_, this, ()| {
            let document = this.layout.borrow();
            let keymap = document.document().clone();
            let adapter: AdapterLayout = keymap.into();
            adapter
                .to_standard_json()
                .map_err(|err| script_error(format!("failed to export JSON: {err}")))
        });

        methods.add_method(
            "render_template",
            |_, _, (json, template_path): (String, String)| {
                let template = fs::read_to_string(&template_path).map_err(|err| {
                    script_error(format!("failed to read {template_path}: {err}"))
                })?;
                render_standard_template(&json, &template)
                    .map_err(|err| script_error(format!("failed to render template: {err}")))
            },
        );

        methods.add_method("build_firmware", |lua, this, opts: mlua::Table| {
            let manifest_path: String = opts
                .get("manifest")
                .map_err(|_| script_error("build_firmware requires `manifest`"))?;
            let manifest = load_manifest_for_lua(&manifest_path)?;
            let builder = FirmwareBuilder::new(manifest, Box::new(CliDockerBackend::new()));
            let bundle = build_request_from_table(&builder, opts.clone(), &this.layout)?;
            let dry_run: bool = opts.get("dry_run").unwrap_or(false);
            if dry_run {
                return render_dry_run(lua, &bundle);
            }
            let report = builder
                .build(bundle.request)
                .map_err(|err| script_error(format!("firmware build failed: {err}")))?;
            render_build_report(lua, report)
        });
    }
}

pub fn install_layout_api(lua: &Lua, layout: SharedLayout, logs: SharedLogs) -> LuaResult<()> {
    let api = LayoutApi::new(layout, logs);
    lua.globals().set("layout", api)?;
    Ok(())
}

fn lua_value_to_toml(value: LuaValue) -> LuaResult<TomlValue> {
    match value {
        LuaValue::Nil => Err(script_error("metadata values cannot be nil")),
        LuaValue::Boolean(flag) => Ok(TomlValue::Boolean(flag)),
        LuaValue::Integer(num) => Ok(TomlValue::Integer(num)),
        LuaValue::Number(num) => Ok(TomlValue::Float(num)),
        LuaValue::String(text) => Ok(TomlValue::String(text.to_str()?.to_string())),
        LuaValue::Table(table) => {
            if table_is_array(&table)? {
                let mut items = Vec::new();
                for entry in table.sequence_values::<LuaValue>() {
                    items.push(lua_value_to_toml(entry?)?);
                }
                Ok(TomlValue::Array(items))
            } else {
                let mut map = TomlMap::new();
                for pair in table.pairs::<LuaValue, LuaValue>() {
                    let (key, entry) = pair?;
                    let key = match key {
                        LuaValue::String(name) => name.to_str()?.to_string(),
                        other => {
                            return Err(script_error(format!(
                                "metadata keys must be strings, found {}",
                                other.type_name()
                            )));
                        }
                    };
                    map.insert(key, lua_value_to_toml(entry)?);
                }
                Ok(TomlValue::Table(map))
            }
        }
        other => Err(script_error(format!(
            "unsupported metadata value type `{}`",
            other.type_name()
        ))),
    }
}

fn table_is_array(table: &LuaTable) -> LuaResult<bool> {
    for pair in table.clone().pairs::<LuaValue, LuaValue>() {
        let (key, _) = pair?;
        if let LuaValue::Integer(index) = key {
            if index >= 1 {
                continue;
            }
        }
        return Ok(false);
    }
    Ok(true)
}

struct RequestBundle {
    request: BuildRequest,
    layout_kind: String,
}

fn load_manifest_for_lua(path: &str) -> LuaResult<crate::build::FirmwareManifest> {
    use std::path::Path;
    if Path::new(path).exists() {
        crate::build::FirmwareManifest::from_file(path)
            .map_err(|err| script_error(format!("failed to load manifest {path}: {err}")))
    } else {
        crate::build::FirmwareManifest::load(path)
            .map_err(|err| script_error(format!("failed to load manifest {path}: {err}")))
    }
}

fn build_request_from_table(
    builder: &FirmwareBuilder,
    opts: mlua::Table,
    layout: &SharedLayout,
) -> LuaResult<RequestBundle> {
    use std::sync::Arc;

    let keyboard: String = opts
        .get("keyboard")
        .map_err(|_| script_error("build_firmware requires `keyboard`"))?;
    let mut req = builder.builder().keyboard(keyboard.clone());

    if let Some(toolchain) = opts.get::<_, Option<String>>("toolchain")? {
        req = req.toolchain(toolchain);
    }

    if let Some(targets) = opts.get::<_, Option<mlua::Table>>("targets")? {
        for value in targets.sequence_values::<String>() {
            let target =
                value.map_err(|err| script_error(format!("invalid target entry: {err}")))?;
            req = req.target(target);
        }
    }

    if let Some(env) = opts.get::<_, Option<mlua::Table>>("env")? {
        for pair in env.pairs::<mlua::Value, mlua::Value>() {
            let (key, value) = pair.map_err(|err| script_error(err.to_string()))?;
            let key = match key {
                mlua::Value::String(s) => s
                    .to_str()
                    .map_err(|err| script_error(err.to_string()))?
                    .to_string(),
                other => {
                    return Err(script_error(format!(
                        "env keys must be strings (got {})",
                        other.type_name()
                    )));
                }
            };
            let value = match value {
                mlua::Value::String(s) => s
                    .to_str()
                    .map_err(|err| script_error(err.to_string()))?
                    .to_string(),
                other => {
                    return Err(script_error(format!(
                        "env values must be strings (got {})",
                        other.type_name()
                    )));
                }
            };
            req = req.env(key, value);
        }
    }

    let output_dir: String = opts
        .get::<_, Option<String>>("output_dir")?
        .unwrap_or_else(|| "out/firmware".to_string());
    let disable_cache: bool = opts.get("disable_cache").unwrap_or(false);
    let verbose: bool = opts.get("verbose").unwrap_or(false);
    let progress: Arc<dyn crate::build::progress::ProgressReporter> = if verbose {
        Arc::new(CliProgressReporter)
    } else {
        Arc::new(NoopProgressReporter)
    };
    req = req
        .output_dir(output_dir)
        .disable_cache(disable_cache)
        .progress(progress);

    let layout_json: Option<String> = opts.get("layout_json")?;
    let layout_json_text: Option<String> = opts.get("layout_json_text")?;
    let layout_dts: Option<String> = opts.get("layout_dts")?;
    let layout_dts_text: Option<String> = opts.get("layout_dts_text")?;
    let keymap: Option<String> = opts.get("keymap")?;
    let kconfig: Option<String> = opts.get("kconfig")?;
    let use_current: bool = opts.get("use_current").unwrap_or(false);

    if kconfig.is_some() && keymap.is_none() {
        return Err(script_error("--kconfig requires `keymap`"));
    }

    let provided_inputs = [
        layout_json.is_some(),
        layout_json_text.is_some(),
        layout_dts.is_some(),
        layout_dts_text.is_some(),
        keymap.is_some(),
        use_current,
    ]
    .iter()
    .filter(|flag| **flag)
    .count();
    if provided_inputs > 1 {
        return Err(script_error(
            "provide only one layout input (JSON/DTS/keymap/use_current)",
        ));
    }

    let mut layout_kind = None;
    if let Some(path) = layout_json {
        req = req.layout_json_path(path.clone());
        layout_kind = Some("layout_json_path".to_string());
    } else if let Some(text) = layout_json_text {
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|err| script_error(format!("layout_json_text is not valid JSON: {err}")))?;
        req = req.layout_json_value(value);
        layout_kind = Some("layout_json_text".to_string());
    } else if let Some(path) = layout_dts {
        let pipeline = AdapterPipeline::from_dts_path(path);
        req = req.layout_via_pipeline(pipeline);
        layout_kind = Some("layout_dts_path".to_string());
    } else if let Some(text) = layout_dts_text {
        let pipeline = AdapterPipeline::from_dts_text(text);
        req = req.layout_via_pipeline(pipeline);
        layout_kind = Some("layout_dts_text".to_string());
    } else if let Some(keymap_path) = keymap {
        let extra = kconfig.as_ref().map(std::path::PathBuf::from);
        req = req.layout_files(keymap_path, extra);
        layout_kind = Some("keymap".to_string());
    } else if use_current {
        let rendered = serialize_keymap(layout.borrow().document().clone())
            .map_err(|err| script_error(format!("failed to render current layout: {err}")))?;
        let doc = DtsDocument::parse_str(&rendered)
            .map_err(|err| script_error(format!("failed to parse rendered layout: {err}")))?;
        req = req.layout_document(doc);
        layout_kind = Some("current_layout".to_string());
    }

    if layout_kind.is_none() {
        return Err(script_error(
            "provide one of layout_json/layout_json_text/layout_dts/layout_dts_text/keymap or set use_current=true",
        ));
    }
    if layout_kind.as_deref() != Some("keymap") && kconfig.is_some() {
        return Err(script_error("--kconfig requires `keymap`"));
    }

    let request = req
        .build()
        .map_err(|err| script_error(format!("invalid build request: {err}")))?;
    Ok(RequestBundle {
        request,
        layout_kind: layout_kind.unwrap_or_else(|| "unknown".to_string()),
    })
}

fn render_request_summary<'lua>(
    lua: &'lua Lua,
    bundle: &RequestBundle,
) -> LuaResult<mlua::Table<'lua>> {
    let summary = lua.create_table()?;
    summary.set("keyboard", bundle.request.keyboard_id.clone())?;
    if let Some(toolchain) = &bundle.request.toolchain_id {
        summary.set("toolchain", toolchain.clone())?;
    }
    let targets = lua.create_table()?;
    for (idx, target) in bundle.request.targets.iter().enumerate() {
        targets.set(idx + 1, target.id.clone())?;
    }
    summary.set("targets", targets)?;
    summary.set(
        "output_dir",
        bundle.request.output_dir.to_string_lossy().to_string(),
    )?;
    summary.set("layout_kind", bundle.layout_kind.clone())?;
    Ok(summary)
}

fn render_dry_run<'lua>(lua: &'lua Lua, bundle: &RequestBundle) -> LuaResult<mlua::Table<'lua>> {
    let table = lua.create_table()?;
    table.set("success", true)?;
    table.set("built", false)?;
    table.set("request", render_request_summary(lua, bundle)?)?;
    Ok(table)
}

fn render_build_report<'lua>(lua: &'lua Lua, report: BuildReport) -> LuaResult<mlua::Table<'lua>> {
    let table = lua.create_table()?;
    table.set("success", report.success)?;
    table.set("built", true)?;
    let artifacts = lua.create_table()?;
    for (idx, path) in report.artifacts.files.iter().enumerate() {
        artifacts.set(idx + 1, path.to_string_lossy().to_string())?;
    }
    table.set("artifacts", artifacts)?;
    if let Some(path) = report.logs_path {
        table.set("logs_path", path.to_string_lossy().to_string())?;
    }
    if let Some(path) = report.build_info_path {
        table.set("build_info_path", path.to_string_lossy().to_string())?;
    }
    let metadata = lua.create_table()?;
    for (key, value) in report.metadata.entries {
        metadata.set(key, value)?;
    }
    table.set("metadata", metadata)?;
    Ok(table)
}
