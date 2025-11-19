use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::error::BuildError;

/// Shared log file used to capture Docker output for a build.
#[derive(Clone)]
pub struct LogFile {
    path: PathBuf,
    file: Arc<Mutex<File>>,
}

impl LogFile {
    pub fn create(path: impl Into<PathBuf>) -> Result<Self, BuildError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(BuildError::Io)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .map_err(BuildError::Io)?;
        Ok(Self {
            path,
            file: Arc::new(Mutex::new(file)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, stream: &str, line: &str) {
        if let Ok(mut guard) = self.file.lock() {
            let _ = writeln!(guard, "[{stream}] {line}");
        }
    }
}
