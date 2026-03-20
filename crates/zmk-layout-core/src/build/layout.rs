use std::{env, fs, path::PathBuf};

use crate::{
    adapters::standard::render_standard_template_for_profile,
    keymap::KeymapDocument,
    layout_handle::{LayoutHandle, LayoutHandleError, LayoutOrigin},
};

use super::{error::BuildError, request::LayoutSource, workspace::WorkspaceHandle};

/// Files emitted for a layout request.
#[derive(Debug, Default, Clone)]
pub struct KeymapArtifacts {
    pub json: Option<PathBuf>,
    pub keymap: Option<PathBuf>,
    pub config: Option<PathBuf>,
}

pub struct LayoutStager;

impl LayoutStager {
    pub fn new() -> Self {
        Self
    }

    pub fn stage(
        &self,
        source: &LayoutSource,
        profile: Option<&crate::build::manifest::KeyboardProfileDocument>,
        workspace: &WorkspaceHandle,
    ) -> Result<KeymapArtifacts, BuildError> {
        match source {
            LayoutSource::JsonPath(path) => {
                if let Some(profile) = profile {
                    let json_text = fs::read_to_string(path).map_err(BuildError::Io)?;
                    let profile_root = profile_root(profile);
                    let rendered = render_standard_template_for_profile(
                        &json_text,
                        &profile.document,
                        &profile_root,
                    )
                    .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                    let rendered = apply_position_macros(rendered, profile);
                    return self.write_rendered_layout(json_text, rendered, workspace);
                }
                let mut handle = LayoutHandle::from_json_path(path, LayoutOrigin::JsonFile)
                    .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                self.write_handle(&mut handle, workspace, true)
            }
            LayoutSource::JsonValue(value) => {
                if let Some(profile) = profile {
                    let json_text = serde_json::to_string(value)
                        .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                    let profile_root = profile_root(profile);
                    let rendered = render_standard_template_for_profile(
                        &json_text,
                        &profile.document,
                        &profile_root,
                    )
                    .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                    let rendered = apply_position_macros(rendered, profile);
                    return self.write_rendered_layout(json_text, rendered, workspace);
                }
                let json_text = serde_json::to_string(value)
                    .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                let mut handle = LayoutHandle::from_json_text(json_text, LayoutOrigin::JsonText)
                    .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                self.write_handle(&mut handle, workspace, true)
            }
            LayoutSource::Document(document) => {
                let mut handle = LayoutHandle {
                    source_path: None,
                    raw_text: None,
                    preprocessed_text: None,
                    keymap: KeymapDocument::from(
                        crate::adapters::standard::AdapterLayout::from_document(&document),
                    ),
                    profile: profile.map(|p| p.document.clone()),
                    origin: LayoutOrigin::Document,
                };
                self.write_handle(&mut handle, workspace, true)
            }
            LayoutSource::Files { keymap, extra } => {
                self.copy_files(keymap, extra.as_ref(), workspace)
            }
            LayoutSource::Pipeline(pipeline) => {
                let layout = pipeline
                    .clone()
                    .load()
                    .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                let mut handle = LayoutHandle {
                    source_path: None,
                    raw_text: None,
                    preprocessed_text: None,
                    keymap: KeymapDocument::from(layout),
                    profile: profile.map(|p| p.document.clone()),
                    origin: LayoutOrigin::Pipeline,
                };
                self.write_handle(&mut handle, workspace, true)
            }
        }
    }

    fn write_handle(
        &self,
        handle: &mut LayoutHandle,
        workspace: &WorkspaceHandle,
        emit_json: bool,
    ) -> Result<KeymapArtifacts, BuildError> {
        let keymap_dest = workspace.layout_dir().join("keymap.dtsi");
        let keymap_text = handle.render_keymap_text().map_err(|err| match err {
            LayoutHandleError::Serialize(se) => BuildError::LayoutSerialize(se),
            other => BuildError::InvalidRequest(other.to_string()),
        })?;
        fs::write(&keymap_dest, keymap_text).map_err(BuildError::Io)?;

        let mut artifacts = KeymapArtifacts {
            keymap: Some(keymap_dest),
            ..Default::default()
        };

        if emit_json {
            let json_dest = workspace.layout_dir().join("layout.json");
            let json = handle
                .render_standard_json()
                .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
            fs::write(&json_dest, json).map_err(BuildError::Io)?;
            artifacts.json = Some(json_dest);
        }

        Ok(artifacts)
    }

    fn write_rendered_layout(
        &self,
        json_text: String,
        keymap_text: String,
        workspace: &WorkspaceHandle,
    ) -> Result<KeymapArtifacts, BuildError> {
        let keymap_dest = workspace.layout_dir().join("keymap.dtsi");
        fs::write(&keymap_dest, keymap_text).map_err(BuildError::Io)?;
        let json_dest = workspace.layout_dir().join("layout.json");
        fs::write(&json_dest, json_text).map_err(BuildError::Io)?;
        Ok(KeymapArtifacts {
            keymap: Some(keymap_dest),
            json: Some(json_dest),
            ..Default::default()
        })
    }

    fn copy_files(
        &self,
        keymap: &PathBuf,
        extra: Option<&PathBuf>,
        workspace: &WorkspaceHandle,
    ) -> Result<KeymapArtifacts, BuildError> {
        let keymap_dest = workspace.layout_dir().join("keymap.dtsi");
        fs::copy(keymap, &keymap_dest).map_err(BuildError::Io)?;
        let mut artifacts = KeymapArtifacts {
            keymap: Some(keymap_dest),
            ..Default::default()
        };
        if let Some(config) = extra {
            let config_dest = workspace.layout_dir().join("config.dtsi");
            fs::copy(config, &config_dest).map_err(BuildError::Io)?;
            artifacts.config = Some(config_dest);
        }
        Ok(artifacts)
    }
}

fn profile_root(profile: &crate::build::manifest::KeyboardProfileDocument) -> PathBuf {
    let template = PathBuf::from(&profile.document.layout.template);
    if template.is_absolute() {
        return PathBuf::from(".");
    }
    if let Some(root) = profile.path.parent() {
        let candidate = root.join(&template);
        if candidate.exists() {
            return root.to_path_buf();
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn apply_position_macros(
    keymap_text: String,
    profile: &crate::build::manifest::KeyboardProfileDocument,
) -> String {
    let Some(map) = load_position_macro_map(profile) else {
        return keymap_text;
    };
    map.into_iter().fold(keymap_text, |mut acc, (name, value)| {
        acc = acc.replace(&name, &value);
        acc
    })
}

fn load_position_macro_map(
    profile: &crate::build::manifest::KeyboardProfileDocument,
) -> Option<Vec<(String, String)>> {
    let dir = profile.path.parent()?;
    let path = dir.join("key_positions.json");
    let contents = fs::read_to_string(path).ok()?;
    let entries: Vec<serde_json::Value> = serde_json::from_str(&contents).ok()?;
    let mut map = Vec::new();
    for entry in entries {
        let Some(label) = entry.get("label").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(index) = entry.get("i").and_then(|v| v.as_u64()) else {
            continue;
        };
        let macro_name = if let Some(stripped) = label.strip_prefix("L_") {
            format!("POS_LH_{stripped}")
        } else if let Some(stripped) = label.strip_prefix("R_") {
            format!("POS_RH_{stripped}")
        } else {
            continue;
        };
        map.push((macro_name, index.to_string()));
    }
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::AdapterPipeline;
    use crate::build::{manifest::CachePolicy, workspace::WorkspaceManager};
    use crate::profiles::KeyboardProfileDoc;
    use serde_json::Value as JsonValue;
    use tempfile::tempdir;

    #[test]
    fn pipeline_stage_writes_keymap_and_json() -> Result<(), BuildError> {
        let manager = WorkspaceManager::new();
        let workspace = manager.create_workspace("test", &CachePolicy::default(), true)?;
        let stager = LayoutStager::new();
        let dts = r#"
/ {
    behaviors {};
    macros {};
    combos {};
};

keymap {
    compatible = "zmk,keymap";
    base {
        bindings = < &none >;
    };
};
"#;
        let pipeline = AdapterPipeline::from_dts_text(dts);

        let artifacts = stager.stage(&LayoutSource::Pipeline(pipeline), None, &workspace)?;
        assert!(artifacts.keymap.as_ref().unwrap().exists());
        assert!(artifacts.json.as_ref().unwrap().exists());
        Ok(())
    }

    #[test]
    fn json_source_stages_keymap_with_profile_template() -> Result<(), BuildError> {
        let tmp = tempdir().expect("tmpdir");
        let template_path = tmp.path().join("layout.dtsi");
        let template_source = r#"
/ {
    behaviors {};
    macros {};
    combos {};
};

keymap {
    compatible = "zmk,keymap";
    base {
        bindings = < &none >;
    };
};
"#;
        fs::write(&template_path, template_source).expect("write template");

        let manager = WorkspaceManager::new();
        let workspace = manager.create_workspace("test", &CachePolicy::default(), true)?;
        let stager = LayoutStager::new();

        let document = crate::dts::DtsDocument::parse_str(template_source).unwrap();
        let adapter_layout = crate::adapters::standard::AdapterLayout::from_document(&document);
        let json_text = adapter_layout.to_standard_json().unwrap();
        let json_value: JsonValue = serde_json::from_str(&json_text).unwrap();

        let profile_toml = format!(
            r#"
keyboard = "tmp"
version = 1

[metadata]
name = "tmp"
vendor = "tmp"

[hardware]
key_count = 1
is_split = false

[firmware]
default = "main"
[firmware.versions.main]
id = "main"
repository = "https://example.com/repo.git"
branch = "main"

[layout]
template = "{}"

[layout.formatting]
rows = [ {{ keys = [0] }} ]
"#,
            template_path.display()
        );
        let doc = KeyboardProfileDoc::from_toml_str(&profile_toml).unwrap();
        let profile = crate::build::manifest::KeyboardProfileDocument {
            path: template_path.clone(),
            document: doc,
        };

        let artifacts = stager.stage(
            &LayoutSource::JsonValue(json_value),
            Some(&profile),
            &workspace,
        )?;
        assert!(artifacts.keymap.as_ref().unwrap().exists());
        assert!(artifacts.json.as_ref().unwrap().exists());
        Ok(())
    }

    #[test]
    fn document_source_stages_keymap_and_json() -> Result<(), BuildError> {
        let manager = WorkspaceManager::new();
        let workspace = manager.create_workspace("test", &CachePolicy::default(), true)?;
        let stager = LayoutStager::new();
        let dts = r#"
/ {
    behaviors {};
    macros {};
    combos {};
};

keymap {
    compatible = "zmk,keymap";
    base {
        bindings = < &none >;
    };
};
"#;
        let document = crate::dts::DtsDocument::parse_str(dts).unwrap();
        let artifacts = stager.stage(&LayoutSource::Document(document), None, &workspace)?;
        assert!(artifacts.keymap.as_ref().unwrap().exists());
        assert!(artifacts.json.as_ref().unwrap().exists());
        Ok(())
    }

    #[test]
    fn files_source_copies_keymap_and_config() -> Result<(), BuildError> {
        let tmp = tempdir().expect("tmpdir");
        let keymap_path = tmp.path().join("keymap.dtsi");
        let config_path = tmp.path().join("config.dtsi");
        fs::write(&keymap_path, "keymap {};\n").unwrap();
        fs::write(&config_path, "CONFIG_FOO=y\n").unwrap();

        let manager = WorkspaceManager::new();
        let workspace = manager.create_workspace("test", &CachePolicy::default(), true)?;
        let stager = LayoutStager::new();
        let artifacts = stager.stage(
            &LayoutSource::Files {
                keymap: keymap_path.clone(),
                extra: Some(config_path.clone()),
            },
            None,
            &workspace,
        )?;
        assert!(artifacts.keymap.as_ref().unwrap().exists());
        assert!(artifacts.config.as_ref().unwrap().exists());
        Ok(())
    }
}
