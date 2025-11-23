use std::{fs, io::Write, path::PathBuf};

use serde_json::Value as JsonValue;

use crate::{
    adapters::AdapterPipeline,
    dts::DtsDocument,
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
        workspace: &WorkspaceHandle,
    ) -> Result<KeymapArtifacts, BuildError> {
        match source {
            LayoutSource::JsonPath(path) => self.copy_json(path, workspace),
            LayoutSource::JsonValue(value) => self.write_json(value, workspace),
            LayoutSource::Document(document) => self.write_document(document, workspace),
            LayoutSource::Files { keymap, extra } => {
                self.copy_files(keymap, extra.as_ref(), workspace)
            }
            LayoutSource::Pipeline(pipeline) => self.write_pipeline(pipeline, workspace),
        }
    }

    fn copy_json(
        &self,
        path: &PathBuf,
        workspace: &WorkspaceHandle,
    ) -> Result<KeymapArtifacts, BuildError> {
        let dest = workspace.layout_dir().join("layout.json");
        fs::copy(path, &dest).map_err(BuildError::Io)?;
        Ok(KeymapArtifacts {
            json: Some(dest),
            ..Default::default()
        })
    }

    fn write_json(
        &self,
        value: &JsonValue,
        workspace: &WorkspaceHandle,
    ) -> Result<KeymapArtifacts, BuildError> {
        let dest = workspace.layout_dir().join("layout.json");
        let mut file = fs::File::create(&dest).map_err(BuildError::Io)?;
        let text = serde_json::to_string_pretty(value)
            .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
        file.write_all(text.as_bytes()).map_err(BuildError::Io)?;
        Ok(KeymapArtifacts {
            json: Some(dest),
            ..Default::default()
        })
    }

    fn write_document(
        &self,
        document: &DtsDocument,
        workspace: &WorkspaceHandle,
    ) -> Result<KeymapArtifacts, BuildError> {
        let dest = workspace.layout_dir().join("keymap.dtsi");
        let text = document.to_string().map_err(BuildError::LayoutSerialize)?;
        fs::write(&dest, text).map_err(BuildError::Io)?;
        Ok(KeymapArtifacts {
            keymap: Some(dest),
            ..Default::default()
        })
    }

    fn write_pipeline(
        &self,
        pipeline: &AdapterPipeline,
        workspace: &WorkspaceHandle,
    ) -> Result<KeymapArtifacts, BuildError> {
        let layout = pipeline
            .clone()
            .load()
            .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
        let json_dest = workspace.layout_dir().join("layout.json");
        let json = layout
            .to_standard_json()
            .map_err(|err| BuildError::InvalidRequest(err.to_string()))?;
        fs::write(&json_dest, json).map_err(BuildError::Io)?;
        Ok(KeymapArtifacts {
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
