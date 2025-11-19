//! Firmware build scaffolding (manifest + request + CLI wiring).

pub mod builder;
pub mod docker;
pub mod error;
pub mod layout;
pub mod logs;
pub mod manifest;
pub mod progress;
pub mod request;
pub mod toolchain;
pub mod toolchains;
pub mod workspace;

pub use builder::{
    ArtifactReport, BuildMetadata, BuildReport, CliProgressReporter, FirmwareBuilder,
};
pub use docker::{
    CliDockerBackend, DockerBackend, DockerBuildOptions, DockerInvocation, DockerUser,
    NullOutputHandler, OutputHandler, ProcessStatus, VolumeMode, VolumeMount,
};
pub use error::BuildError;
pub use layout::{KeymapArtifacts, LayoutStager};
pub use manifest::{
    BuildTarget, CacheMode, CachePath, CachePolicy, FirmwareManifest, KeyboardProfile,
    ManifestError, ToolchainKind, ToolchainOverride, ToolchainProfile,
};
pub use progress::{LogLevel, NoopProgressReporter, ProgressReporter};
pub use request::{
    BuildRequest, BuildRequestBuilder, BuildRequestError, BuildTargetRef, LayoutSource,
};
pub use toolchain::{BuildContext, ToolchainRunResult};
pub use workspace::{WorkspaceHandle, WorkspaceManager};
