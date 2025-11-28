use std::cell::RefCell;
use std::fs;
use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use super::{
    behavior::BehaviorObject,
    combo::ComboObject,
    conditional::ConditionalObject,
    input::InputObject,
    layer::LayerBuilder,
    macro_builder::MacroObject,
    query::{BehaviorInfo, ComboInfo, LayerInfo, list_behavior_definitions, list_combo_definitions},
    util::{SharedLayout, SharedLogs, script_error},
};

use zmk_layout_core::{
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
use toml::{Value as TomlValue, map::Map as TomlMap};

/// Main Layout API object for ZMK layout manipulation.
///
/// This class provides methods to load, modify, and export ZMK keyboard layouts.
/// It follows a fluent builder pattern for modifications.
#[pyclass(unsendable)]
#[derive(Clone)]
pub struct Layout {
    layout: SharedLayout,
    #[allow(dead_code)]
    logs: SharedLogs,
}

#[pymethods]
impl Layout {
    /// Create a new empty Layout.
    #[new]
    fn new() -> Self {
        let engine = LayoutEngine::empty();
        let shared_layout = Arc::new(RefCell::new(engine));
        let shared_logs: Arc<RefCell<Vec<String>>> = Arc::new(RefCell::new(Vec::new()));
        Self {
            layout: shared_layout,
            logs: shared_logs,
        }
    }

    /// Get a layer builder for the given layer name.
    ///
    /// Args:
    ///     name: The name of the layer to modify or create.
    ///
    /// Returns:
    ///     A LayerBuilder for fluent configuration.
    fn layer(&self, name: String) -> LayerBuilder {
        LayerBuilder::new(name, Arc::clone(&self.layout))
    }

    /// Get a combo builder for the given combo name.
    ///
    /// Args:
    ///     name: The name of the combo to modify or create.
    ///
    /// Returns:
    ///     A ComboObject for fluent configuration.
    fn combo(&self, name: String) -> ComboObject {
        ComboObject::new(name, Arc::clone(&self.layout))
    }

    /// Get a behavior builder for the given behavior name.
    ///
    /// Args:
    ///     name: The name of the behavior to modify or create.
    ///
    /// Returns:
    ///     A BehaviorObject for fluent configuration.
    fn behavior(&self, name: String) -> BehaviorObject {
        BehaviorObject::new(name, Arc::clone(&self.layout))
    }

    /// Get a macro builder for the given macro name.
    ///
    /// Args:
    ///     name: The name of the macro to modify or create.
    ///
    /// Returns:
    ///     A MacroObject for fluent configuration.
    #[pyo3(name = "macro_")]
    fn macro_builder(&self, name: String) -> MacroObject {
        MacroObject::new(name, Arc::clone(&self.layout))
    }

    /// Get an input builder for encoders/sensors.
    ///
    /// Args:
    ///     name: The name of the input device.
    ///
    /// Returns:
    ///     An InputObject for fluent configuration.
    fn input(&self, name: String) -> InputObject {
        InputObject::new(name, Arc::clone(&self.layout))
    }

    /// Get a conditional builder.
    ///
    /// Args:
    ///     name: The name of the conditional.
    ///
    /// Returns:
    ///     A ConditionalObject for fluent configuration.
    fn conditional(&self, name: String) -> ConditionalObject {
        ConditionalObject::new(name, Arc::clone(&self.layout))
    }

    /// Reset to an empty layout.
    fn clear(&self) {
        *self.layout.borrow_mut() = LayoutEngine::empty();
    }

    /// Move a layer to a new position in the layer order.
    ///
    /// Args:
    ///     name: The name of the layer to move.
    ///     index: The new position (1-based index).
    fn move_layer(&self, name: String, index: i64) -> PyResult<()> {
        if index < 1 {
            return Err(script_error("layer index must be >= 1 (1-based indexing)"));
        }
        let normalized = (index - 1) as usize;
        self.layout
            .borrow_mut()
            .reorder_layer(&name, normalized)
            .map_err(|err| script_error(err.to_string()))
    }

    /// Remove a layer by name.
    ///
    /// Args:
    ///     name: The name of the layer to remove.
    fn remove_layer(&self, name: String) -> PyResult<()> {
        self.layout
            .borrow_mut()
            .remove_layer(&name)
            .map_err(|err| script_error(err.to_string()))
    }

    /// Set a metadata entry on the layout.
    ///
    /// Args:
    ///     key: The metadata key.
    ///     value: The metadata value (string, int, float, bool, list, or dict).
    fn meta(&self, key: String, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let toml = pyany_to_toml(value)?;
        self.layout
            .borrow_mut()
            .set_meta_entry(&key, &toml)
            .map_err(|err| script_error(err.to_string()))
    }

    /// Get information about a specific layer.
    ///
    /// Args:
    ///     name: The name of the layer.
    ///
    /// Returns:
    ///     LayerInfo if the layer exists, None otherwise.
    fn get_layer(&self, name: String) -> PyResult<Option<LayerInfo>> {
        let engine = self.layout.borrow();
        LayerInfo::from_engine(&engine, &name)
    }

    /// Get information about a specific combo.
    ///
    /// Args:
    ///     name: The name of the combo.
    ///
    /// Returns:
    ///     ComboInfo if the combo exists, None otherwise.
    fn get_combo(&self, name: String) -> Option<ComboInfo> {
        let engine = self.layout.borrow();
        let def = list_combo_definitions(&engine)
            .into_iter()
            .find(|combo| combo.name == name);
        def.map(ComboInfo::from_definition)
    }

    /// Get information about a specific behavior.
    ///
    /// Args:
    ///     name: The name of the behavior.
    ///
    /// Returns:
    ///     BehaviorInfo if the behavior exists, None otherwise.
    fn get_behavior(&self, name: String) -> Option<BehaviorInfo> {
        let engine = self.layout.borrow();
        let def = list_behavior_definitions(&engine)
            .into_iter()
            .find(|behavior| behavior.name == name);
        def.map(BehaviorInfo::from_definition)
    }

    /// List all layer names in order.
    ///
    /// Returns:
    ///     List of layer names.
    fn list_layers(&self) -> Vec<String> {
        let engine = self.layout.borrow();
        engine.layer_names()
    }

    /// List all combo names.
    ///
    /// Returns:
    ///     List of combo names.
    fn list_combos(&self) -> Vec<String> {
        let engine = self.layout.borrow();
        list_combo_definitions(&engine)
            .into_iter()
            .map(|combo| combo.name)
            .collect()
    }

    /// List all behavior names.
    ///
    /// Returns:
    ///     List of behavior names.
    fn list_behaviors(&self) -> Vec<String> {
        let engine = self.layout.borrow();
        list_behavior_definitions(&engine)
            .into_iter()
            .map(|behavior| behavior.name)
            .collect()
    }

    /// Load a DTS/DTSI keymap file.
    ///
    /// Args:
    ///     path: Path to the keymap file.
    fn load_dts(&self, path: String) -> PyResult<()> {
        let text = fs::read_to_string(&path)
            .map_err(|err| script_error(format!("failed to read {path}: {err}")))?;
        let doc = DtsDocument::parse_str(&text)
            .map_err(|err| script_error(format!("failed to parse {path}: {err}")))?;
        let keymap = KeymapDocument::from_document(doc);
        *self.layout.borrow_mut() = LayoutEngine::new(keymap);
        Ok(())
    }

    /// Load a DTS/DTSI keymap file (alias for load_dts).
    ///
    /// Args:
    ///     path: Path to the keymap file.
    fn load_dtsi(&self, path: String) -> PyResult<()> {
        self.load_dts(path)
    }

    /// Load a JSON layout file with a template.
    ///
    /// Args:
    ///     json_path: Path to the JSON layout file.
    ///     template_path: Path to the DTS template file.
    fn load_json(&self, json_path: String, template_path: String) -> PyResult<()> {
        let doc = import_standard_file_with_template(&json_path, &template_path)
            .map_err(|err| script_error(format!("failed to import {json_path}: {err}")))?;
        let keymap = KeymapDocument::from_document(doc);
        *self.layout.borrow_mut() = LayoutEngine::new(keymap);
        Ok(())
    }

    /// Save the layout as a DTS/DTSI file.
    ///
    /// Args:
    ///     path: Path to write the keymap file.
    fn save_dts(&self, path: String) -> PyResult<()> {
        let keymap = self.layout.borrow().document().clone();
        let rendered = serialize_keymap(keymap)
            .map_err(|err| script_error(format!("failed to render DTS: {err}")))?;
        fs::write(&path, rendered)
            .map_err(|err| script_error(format!("failed to write {path}: {err}")))
    }

    /// Save the layout as a DTS/DTSI file (alias for save_dts).
    ///
    /// Args:
    ///     path: Path to write the keymap file.
    fn save_dtsi(&self, path: String) -> PyResult<()> {
        self.save_dts(path)
    }

    /// Save the layout as a JSON file.
    ///
    /// Args:
    ///     path: Path to write the JSON file.
    fn save_json(&self, path: String) -> PyResult<()> {
        let document = self.layout.borrow();
        let keymap = document.document().clone();
        let adapter: AdapterLayout = keymap.into();
        let json = adapter
            .to_standard_json()
            .map_err(|err| script_error(format!("failed to export JSON: {err}")))?;
        fs::write(&path, json).map_err(|err| script_error(format!("failed to write {path}: {err}")))
    }

    /// Parse a DTS string directly.
    ///
    /// Args:
    ///     source: DTS source code as a string.
    fn parse_dts(&self, source: String) -> PyResult<()> {
        let doc = DtsDocument::parse_str(&source)
            .map_err(|err| script_error(format!("failed to parse DTS: {err}")))?;
        let keymap = KeymapDocument::from_document(doc);
        *self.layout.borrow_mut() = LayoutEngine::new(keymap);
        Ok(())
    }

    /// Parse a JSON string with a template file.
    ///
    /// Args:
    ///     json: JSON source code as a string.
    ///     template_path: Path to the DTS template file.
    fn parse_json(&self, json: String, template_path: String) -> PyResult<()> {
        let template = fs::read_to_string(&template_path)
            .map_err(|err| script_error(format!("failed to read {template_path}: {err}")))?;
        let doc = import_standard_str_with_template(&json, &template)
            .map_err(|err| script_error(format!("failed to parse JSON: {err}")))?;
        *self.layout.borrow_mut() = LayoutEngine::new(KeymapDocument::from_document(doc));
        Ok(())
    }

    /// Convert the layout to a DTS string.
    ///
    /// Returns:
    ///     The layout as DTS source code.
    fn to_dts_string(&self) -> PyResult<String> {
        let keymap = self.layout.borrow().document().clone();
        serialize_keymap(keymap).map_err(|err| script_error(format!("failed to serialize DTS: {err}")))
    }

    /// Convert the layout to a JSON string.
    ///
    /// Returns:
    ///     The layout as JSON.
    fn to_json_string(&self) -> PyResult<String> {
        let document = self.layout.borrow();
        let keymap = document.document().clone();
        let adapter: AdapterLayout = keymap.into();
        adapter
            .to_standard_json()
            .map_err(|err| script_error(format!("failed to export JSON: {err}")))
    }

    /// Render a JSON layout using a template.
    ///
    /// Args:
    ///     json: JSON layout data as a string.
    ///     template_path: Path to the DTS template file.
    ///
    /// Returns:
    ///     The rendered DTS output.
    fn render_template(&self, json: String, template_path: String) -> PyResult<String> {
        let template = fs::read_to_string(&template_path)
            .map_err(|err| script_error(format!("failed to read {template_path}: {err}")))?;
        render_standard_template(&json, &template)
            .map_err(|err| script_error(format!("failed to render template: {err}")))
    }

    /// Build firmware with the current layout.
    ///
    /// Args:
    ///     manifest: Path to the firmware manifest file.
    ///     keyboard: Keyboard ID from the manifest.
    ///     **kwargs: Additional options (targets, toolchain, output_dir, etc.)
    ///
    /// Returns:
    ///     Dictionary with build results.
    #[pyo3(signature = (manifest, keyboard, **kwargs))]
    fn build_firmware(
        &self,
        py: Python<'_>,
        manifest: String,
        keyboard: String,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let manifest_obj = load_manifest_for_python(&manifest)?;
        let builder = FirmwareBuilder::new(manifest_obj, Box::new(CliDockerBackend::new()));
        let bundle = build_request_from_kwargs(&builder, keyboard, kwargs, &self.layout)?;

        let dry_run = kwargs
            .and_then(|d| d.get_item("dry_run").ok().flatten())
            .and_then(|v| v.extract::<bool>().ok())
            .unwrap_or(false);

        if dry_run {
            return render_dry_run(py, &bundle);
        }

        let report = builder
            .build(bundle.request)
            .map_err(|err| script_error(format!("firmware build failed: {err}")))?;
        render_build_report(py, report)
    }
}

impl Layout {
    pub fn shared_layout(&self) -> SharedLayout {
        Arc::clone(&self.layout)
    }

    pub fn shared_logs(&self) -> SharedLogs {
        Arc::clone(&self.logs)
    }
}

fn pyany_to_toml(value: &Bound<'_, PyAny>) -> PyResult<TomlValue> {
    if value.is_none() {
        return Err(script_error("metadata values cannot be None"));
    }
    if let Ok(flag) = value.extract::<bool>() {
        return Ok(TomlValue::Boolean(flag));
    }
    if let Ok(num) = value.extract::<i64>() {
        return Ok(TomlValue::Integer(num));
    }
    if let Ok(num) = value.extract::<f64>() {
        return Ok(TomlValue::Float(num));
    }
    if let Ok(text) = value.extract::<String>() {
        return Ok(TomlValue::String(text));
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut items = Vec::new();
        for item in list.iter() {
            items.push(pyany_to_toml(&item)?);
        }
        return Ok(TomlValue::Array(items));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut map = TomlMap::new();
        for (key, val) in dict.iter() {
            let key: String = key.extract()?;
            map.insert(key, pyany_to_toml(&val)?);
        }
        return Ok(TomlValue::Table(map));
    }
    Err(script_error(format!(
        "unsupported metadata value type: {}",
        value.get_type().name()?
    )))
}

struct RequestBundle {
    request: BuildRequest,
    layout_kind: String,
}

fn load_manifest_for_python(path: &str) -> PyResult<zmk_layout_core::build::FirmwareManifest> {
    use std::path::Path;
    if Path::new(path).exists() {
        zmk_layout_core::build::FirmwareManifest::from_file(path)
            .map_err(|err| script_error(format!("failed to load manifest {path}: {err}")))
    } else {
        zmk_layout_core::build::FirmwareManifest::load(path)
            .map_err(|err| script_error(format!("failed to load manifest {path}: {err}")))
    }
}

fn build_request_from_kwargs(
    builder: &FirmwareBuilder,
    keyboard: String,
    kwargs: Option<&Bound<'_, PyDict>>,
    layout: &SharedLayout,
) -> PyResult<RequestBundle> {
    use std::sync::Arc as StdArc;

    let mut req = builder.builder().keyboard(keyboard.clone());

    if let Some(opts) = kwargs {
        if let Some(toolchain) = opts
            .get_item("toolchain")?
            .and_then(|v| v.extract::<String>().ok())
        {
            req = req.toolchain(toolchain);
        }

        if let Some(targets) = opts.get_item("targets")? {
            if let Ok(list) = targets.downcast::<PyList>() {
                for item in list.iter() {
                    let target: String = item.extract()?;
                    req = req.target(target);
                }
            }
        }

        if let Some(env) = opts.get_item("env")? {
            if let Ok(dict) = env.downcast::<PyDict>() {
                for (key, value) in dict.iter() {
                    let key: String = key.extract()?;
                    let value: String = value.extract()?;
                    req = req.env(key, value);
                }
            }
        }
    }

    let output_dir = kwargs
        .and_then(|d| d.get_item("output_dir").ok().flatten())
        .and_then(|v| v.extract::<String>().ok())
        .unwrap_or_else(|| "out/firmware".to_string());

    let disable_cache = kwargs
        .and_then(|d| d.get_item("disable_cache").ok().flatten())
        .and_then(|v| v.extract::<bool>().ok())
        .unwrap_or(false);

    let verbose = kwargs
        .and_then(|d| d.get_item("verbose").ok().flatten())
        .and_then(|v| v.extract::<bool>().ok())
        .unwrap_or(false);

    let progress: StdArc<dyn zmk_layout_core::build::progress::ProgressReporter> = if verbose {
        StdArc::new(CliProgressReporter)
    } else {
        StdArc::new(NoopProgressReporter)
    };

    req = req
        .output_dir(output_dir)
        .disable_cache(disable_cache)
        .progress(progress);

    let layout_json: Option<String> = kwargs
        .and_then(|d| d.get_item("layout_json").ok().flatten())
        .and_then(|v| v.extract().ok());
    let layout_json_text: Option<String> = kwargs
        .and_then(|d| d.get_item("layout_json_text").ok().flatten())
        .and_then(|v| v.extract().ok());
    let layout_dts: Option<String> = kwargs
        .and_then(|d| d.get_item("layout_dts").ok().flatten())
        .and_then(|v| v.extract().ok());
    let layout_dts_text: Option<String> = kwargs
        .and_then(|d| d.get_item("layout_dts_text").ok().flatten())
        .and_then(|v| v.extract().ok());
    let keymap: Option<String> = kwargs
        .and_then(|d| d.get_item("keymap").ok().flatten())
        .and_then(|v| v.extract().ok());
    let kconfig: Option<String> = kwargs
        .and_then(|d| d.get_item("kconfig").ok().flatten())
        .and_then(|v| v.extract().ok());
    let use_current: bool = kwargs
        .and_then(|d| d.get_item("use_current").ok().flatten())
        .and_then(|v| v.extract().ok())
        .unwrap_or(false);

    if kconfig.is_some() && keymap.is_none() {
        return Err(script_error("kconfig requires keymap"));
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
            "provide one of layout_json/layout_json_text/layout_dts/layout_dts_text/keymap or set use_current=True",
        ));
    }

    if layout_kind.as_deref() != Some("keymap") && kconfig.is_some() {
        return Err(script_error("kconfig requires keymap"));
    }

    let request = req
        .build()
        .map_err(|err| script_error(format!("invalid build request: {err}")))?;

    Ok(RequestBundle {
        request,
        layout_kind: layout_kind.unwrap_or_else(|| "unknown".to_string()),
    })
}

fn render_dry_run(py: Python<'_>, bundle: &RequestBundle) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    dict.set_item("success", true)?;
    dict.set_item("built", false)?;

    let request_dict = PyDict::new(py);
    request_dict.set_item("keyboard", bundle.request.keyboard_id.clone())?;
    if let Some(toolchain) = &bundle.request.toolchain_id {
        request_dict.set_item("toolchain", toolchain.clone())?;
    }
    let targets: Vec<String> = bundle.request.targets.iter().map(|t| t.id.clone()).collect();
    request_dict.set_item("targets", targets)?;
    request_dict.set_item(
        "output_dir",
        bundle.request.output_dir.to_string_lossy().to_string(),
    )?;
    request_dict.set_item("layout_kind", bundle.layout_kind.clone())?;

    dict.set_item("request", request_dict)?;
    Ok(dict.unbind().into())
}

fn render_build_report(py: Python<'_>, report: BuildReport) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    dict.set_item("success", report.success)?;
    dict.set_item("built", true)?;

    let artifacts: Vec<String> = report
        .artifacts
        .files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    dict.set_item("artifacts", artifacts)?;

    if let Some(path) = report.logs_path {
        dict.set_item("logs_path", path.to_string_lossy().to_string())?;
    }
    if let Some(path) = report.build_info_path {
        dict.set_item("build_info_path", path.to_string_lossy().to_string())?;
    }

    let metadata = PyDict::new(py);
    for (key, value) in report.metadata.entries {
        metadata.set_item(key, value)?;
    }
    dict.set_item("metadata", metadata)?;

    Ok(dict.unbind().into())
}
