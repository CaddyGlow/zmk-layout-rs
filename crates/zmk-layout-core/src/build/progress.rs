//! Progress + logging interface for firmware builds.

/// Logging levels forwarded to [`ProgressReporter::log`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Listener trait that receives structured progress events.
pub trait ProgressReporter: Send + Sync {
    fn log(&self, _level: LogLevel, _message: &str) {}
    fn start_checkpoint(&self, _id: &str, _message: &str) {}
    fn complete_checkpoint(&self, _id: &str) {}
    fn fail_checkpoint(&self, _id: &str) {}
    fn update_progress(&self, _current: u32, _total: u32, _status: &str) {}
}

/// Basic reporter that ignores all events.
#[derive(Debug, Default)]
pub struct NoopProgressReporter;

impl NoopProgressReporter {
    pub fn new() -> Self {
        Self
    }
}

impl ProgressReporter for NoopProgressReporter {}
