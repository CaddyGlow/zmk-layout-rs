use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    layout_handle::{LayoutHandle, LayoutHandleError, LayoutOrigin, TemplateContext},
    profiles::KeyboardProfileDoc,
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
                let template = self.require_template(profile)?;
                let mut handle = LayoutHandle::from_json_path(
                    path,
                    TemplateContext {
                        source: Some(template),
                        mode: Default::default(),
                    },
                )
                .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                self.write_handle(&mut handle, workspace, true)
            }
            LayoutSource::JsonValue(value) => {
                let template = self.require_template(profile)?;
                let json_text = serde_json::to_string(value)
                    .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                let mut handle = LayoutHandle::from_json_text(
                    json_text,
                    TemplateContext {
                        source: Some(template),
                        mode: Default::default(),
                    },
                    LayoutOrigin::JsonText,
                )
                .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
                self.write_handle(&mut handle, workspace, true)
            }
            LayoutSource::Document(document) => {
                let mut handle = LayoutHandle {
                    source_path: None,
                    raw_text: None,
                    preprocessed_text: None,
                    document: document.clone(),
                    adapter_layout: None,
                    profile: profile.map(|p| p.document.clone()),
                    template_source: None,
                    template_mode: Default::default(),
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
                let template_source = pipeline.template_source_ref().cloned().or_else(|| {
                    profile.and_then(|p| self.load_profile_template(&p.document, Some(&p.path)))
                });
                let template_mode = pipeline.template_mode_value();
                let template = template_source.ok_or_else(|| {
                    BuildError::InvalidRequest("template required for pipeline input".into())
                })?;
                let mut handle = LayoutHandle::from_adapter_layout(
                    layout,
                    TemplateContext {
                        source: Some(template),
                        mode: template_mode,
                    },
                    None,
                    LayoutOrigin::Pipeline,
                )
                .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
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

    fn require_template(
        &self,
        profile: Option<&crate::build::manifest::KeyboardProfileDocument>,
    ) -> Result<String, BuildError> {
        profile
            .and_then(|p| self.load_profile_template(&p.document, Some(&p.path)))
            .ok_or_else(|| {
                BuildError::InvalidRequest("template required for JSON layout input".into())
            })
    }

    fn load_profile_template(
        &self,
        profile: &KeyboardProfileDoc,
        profile_path: Option<&Path>,
    ) -> Option<String> {
        let template = PathBuf::from(&profile.layout.template);
        let resolved = if template.is_absolute() {
            template
        } else if let Some(path) = profile_path {
            let base = path.parent().unwrap_or_else(|| Path::new("."));
            base.join(template)
        } else {
            template
        };
        fs::read_to_string(resolved).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{AdapterPipeline, standard::TemplateParseMode};
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
        let pipeline = AdapterPipeline::from_dts_text(dts)
            .template_source(dts.to_string())
            .template_mode(TemplateParseMode::FullDocument);

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
