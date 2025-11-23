use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::{adapters::TemplateParseMode, flash::FlashSideSelection, tasks::ConflictPolicy};

#[cfg(feature = "ancpp-preprocessor")]
#[derive(Args, Clone, Default)]
pub struct PreprocessorArgs {
    #[arg(long, help = "Preprocess layout files with ancpp before parsing")]
    pub preprocess: bool,
    #[arg(
        long = "cpp-include",
        value_name = "DIR",
        help = "User include directory forwarded to ancpp (repeatable)",
        num_args = 0..
    )]
    pub include: Vec<PathBuf>,
    #[arg(
        long = "cpp-system-include",
        value_name = "DIR",
        help = "System include directory forwarded to ancpp (repeatable)",
        num_args = 0..
    )]
    pub system_include: Vec<PathBuf>,
    #[arg(
        long = "cpp-define",
        value_name = "NAME[=VALUE]",
        help = "Predefine a macro for preprocessing (repeatable)",
        num_args = 0..
    )]
    pub define: Vec<String>,
    #[arg(
        long = "no-cpp-relative",
        help = "Disable resolving relative #include paths from the source file location"
    )]
    pub no_resolve_relative: bool,
}

#[derive(Parser)]
#[command(name = "zmk-layout", version, about = "Layout customization CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn parse() -> Self {
        <Self as Parser>::parse()
    }
}

#[derive(Subcommand)]
pub enum Command {
    Apply(ApplyArgs),
    Validate(ValidateArgs),
    Diff(DiffArgs),
    Script(ScriptArgs),
    #[command(subcommand)]
    Firmware(FirmwareCommand),
    #[command(subcommand)]
    Profiles(ProfilesCommand),
    #[command(subcommand)]
    Layer(LayerCommand),
    #[command(subcommand)]
    Bundle(BundleCommand),
}

#[derive(Args, Clone)]
pub struct SharedArgs {
    #[arg(long, value_name = "FILE", help = "Task file to execute (TOML)")]
    pub tasks: PathBuf,
    #[arg(
        long = "base-layout",
        value_name = "DTS",
        help = "Base layout file to transform"
    )]
    pub base_layout: PathBuf,
    #[arg(
        long = "base-template",
        value_name = "NAME",
        help = "Documented template name for warnings"
    )]
    pub base_template: Option<String>,
    #[arg(
        long = "base-version",
        value_name = "VERSION",
        help = "Documented template version for warnings"
    )]
    pub base_version: Option<String>,
    #[arg(
        long = "conflicts",
        value_enum,
        help = "Override the default conflict policy (prompt/override/skip/script)"
    )]
    pub conflicts: Option<ConflictFlag>,
    #[arg(
        long = "combo-conditions",
        help = "Print a summary of combo conditions after task execution"
    )]
    pub combo_conditions: bool,
    #[cfg(feature = "ancpp-preprocessor")]
    #[command(flatten)]
    pub preprocess: PreprocessorArgs,
}

#[derive(Args, Clone)]
pub struct ApplyArgs {
    #[command(flatten)]
    pub shared: SharedArgs,
    #[arg(long, value_name = "FILE", help = "Write updated layout to this file")]
    pub output: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct ValidateArgs {
    #[command(flatten)]
    pub shared: SharedArgs,
}

#[derive(Args, Clone)]
pub struct DiffArgs {
    #[command(flatten)]
    pub shared: SharedArgs,
}

#[derive(Args, Clone)]
pub struct ScriptArgs {
    #[arg(long, value_name = "FILE", help = "Lua script file to execute")]
    pub script: PathBuf,
    #[arg(long = "layout", value_name = "DTS", help = "Layout file to transform")]
    pub layout: PathBuf,
    #[arg(long, value_name = "FILE", help = "Write updated layout to this file")]
    pub output: Option<PathBuf>,
    #[arg(long = "diff", help = "Show diff instead of writing output")]
    pub show_diff: bool,
    #[cfg(feature = "ancpp-preprocessor")]
    #[command(flatten)]
    pub preprocess: PreprocessorArgs,
}

#[derive(Subcommand)]
pub enum FirmwareCommand {
    Build(FirmwareBuildArgs),
    Flash(FirmwareFlashArgs),
    Devices(FirmwareDevicesArgs),
}

#[derive(Subcommand)]
pub enum ProfilesCommand {
    Check(ProfileCheckArgs),
}

#[derive(Subcommand)]
pub enum LayerCommand {
    Export(LayerExportArgs),
    Import(LayerImportArgs),
}

#[derive(Subcommand)]
pub enum BundleCommand {
    Import(BundleImportArgs),
    Export(BundleExportArgs),
    Render(BundleRenderArgs),
}

#[derive(Args, Clone)]
pub struct BundleImportArgs {
    #[arg(value_enum, long, default_value_t = BundleFormat::Moergo, help = "Source layout format")]
    pub format: BundleFormat,
    #[arg(long, value_name = "FILE", help = "Input layout file")]
    pub input: PathBuf,
    #[arg(long, value_name = "FILE", help = "Destination bundle JSON")]
    pub output: PathBuf,
}

#[derive(Args, Clone)]
pub struct BundleExportArgs {
    #[arg(value_enum, long, default_value_t = BundleFormat::Moergo, help = "Export layout format")]
    pub format: BundleFormat,
    #[arg(long, value_name = "FILE", help = "Layout bundle JSON")]
    pub bundle: PathBuf,
    #[arg(long, value_name = "FILE", help = "Destination file to write")]
    pub output: PathBuf,
}

#[derive(Args, Clone)]
pub struct BundleRenderArgs {
    #[arg(long, value_name = "FILE", help = "Layout bundle JSON")]
    pub bundle: PathBuf,
    #[arg(long, value_name = "TARGET", help = "Target id inside the bundle")]
    pub target: String,
    #[arg(long, value_name = "FILE", help = "Override template path")]
    pub template: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Write rendered DTS to this file; prints to stdout when omitted"
    )]
    pub output: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum BundleFormat {
    Moergo,
}

#[derive(Args, Clone)]
pub struct ProfileCheckArgs {
    #[arg(
        value_name = "FILE",
        help = "Keyboard profile TOML file",
        num_args = 0..
    )]
    pub paths: Vec<PathBuf>,
    #[arg(long, help = "Validate every profile under --profiles-dir")]
    pub all: bool,
    #[arg(
        long = "profiles-dir",
        value_name = "DIR",
        default_value = "profiles/keyboards",
        help = "Directory scanned when --all is provided"
    )]
    pub profiles_dir: PathBuf,
}

#[derive(Args, Clone)]
pub struct LayerExportArgs {
    #[arg(long, value_name = "FILE", help = "Input DTS/.dtsi file to parse")]
    pub dts: PathBuf,
    #[arg(long, value_name = "FILE", help = "Destination JSON file to write")]
    pub json: PathBuf,
    #[arg(
        long,
        value_enum,
        help = "Use vendor-specific regex extraction instead of a template"
    )]
    pub vendor: Option<VendorExtractionFlag>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Optional template to extract metadata placeholders"
    )]
    pub template: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = TemplateModeFlag::Strip,
        help = "How to parse the DTS when a template is provided"
    )]
    pub template_mode: TemplateModeFlag,
    #[cfg(feature = "ancpp-preprocessor")]
    #[command(flatten)]
    pub preprocess: PreprocessorArgs,
}

#[derive(Args, Clone)]
pub struct LayerImportArgs {
    #[arg(long, value_name = "FILE", help = "Standard JSON layout file")]
    pub json: PathBuf,
    #[arg(
        long,
        value_name = "FILE",
        help = "DTS template that provides macros, includes, etc."
    )]
    pub template: PathBuf,
    #[arg(long, value_name = "FILE", help = "Output DTS path to write")]
    pub output: PathBuf,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum VendorExtractionFlag {
    Moergo,
}

#[derive(Args, Clone)]
pub struct FirmwareBuildArgs {
    #[arg(long, value_name = "FILE", help = "Firmware manifest (TOML)")]
    pub manifest: PathBuf,
    #[arg(
        long,
        value_name = "KEYBOARD",
        help = "Keyboard id declared in the manifest"
    )]
    pub keyboard: String,
    #[arg(long, value_name = "TOOLCHAIN", help = "Override toolchain id")]
    pub toolchain: Option<String>,
    #[arg(
        long = "target",
        value_name = "ID",
        help = "Target id to build (repeatable)"
    )]
    pub targets: Vec<String>,
    #[arg(
        long = "layout-json",
        value_name = "FILE",
        help = "Layout JSON file to consume"
    )]
    pub layout_json: Option<PathBuf>,
    #[arg(
        long = "layout-dts",
        value_name = "FILE",
        help = "DTS layout to use as input"
    )]
    pub layout_dts: Option<PathBuf>,
    #[arg(
        long = "keymap",
        value_name = "FILE",
        help = "Keymap Devicetree source (.keymap/.dtsi)"
    )]
    pub keymap: Option<PathBuf>,
    #[arg(
        long = "kconfig",
        value_name = "FILE",
        help = "Optional CONFIG overlay (.conf/.config.dtsi)"
    )]
    pub kconfig: Option<PathBuf>,
    #[arg(
        long = "output-dir",
        value_name = "DIR",
        help = "Directory where artifacts should land"
    )]
    pub output_dir: PathBuf,
    #[arg(
        long = "env",
        value_name = "KEY=VALUE",
        help = "Extra env vars forwarded to the toolchain"
    )]
    pub env: Vec<String>,
    #[arg(long, help = "Disable workspace/build cache hydration")]
    pub disable_cache: bool,
    #[arg(
        long,
        help = "Only print the resolved firmware request without running Docker"
    )]
    pub dry_run: bool,
    #[cfg(feature = "ancpp-preprocessor")]
    #[command(flatten)]
    pub preprocess: PreprocessorArgs,
}

#[derive(Args, Clone)]
pub struct FirmwareFlashArgs {
    #[arg(long, value_name = "FILE", help = "Firmware manifest (TOML)")]
    pub manifest: PathBuf,
    #[arg(
        long,
        value_name = "KEYBOARD",
        help = "Keyboard id declared in the manifest"
    )]
    pub keyboard: String,
    #[arg(
        long,
        value_enum,
        value_name = "SIDE",
        help = "Side to flash (default inferred from keyboard profile)"
    )]
    pub side: Option<FlashSideFlag>,
    #[arg(long, value_name = "FILE", help = "UF2 file to flash on every side")]
    pub firmware: Option<PathBuf>,
    #[arg(long, value_name = "FILE", help = "UF2 file to flash on the left half")]
    pub left: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "UF2 file to flash on the right half"
    )]
    pub right: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Build info JSON produced by `zmk-layout firmware build`"
    )]
    pub build_info: Option<PathBuf>,
    #[arg(
        long,
        value_name = "DIR",
        help = "Directory containing UF2 artifacts (auto-picks left/right hints)"
    )]
    pub artifacts: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Mounted bootloader volume to write the UF2 into"
    )]
    pub device: Option<PathBuf>,
    #[arg(
        long,
        value_name = "SECONDS",
        help = "Override the hardware.flash mount_timeout (seconds)"
    )]
    pub mount_timeout: Option<u64>,
    #[arg(
        long,
        value_name = "SECONDS",
        help = "Override the hardware.flash copy_timeout (seconds)"
    )]
    pub copy_timeout: Option<u64>,
    #[arg(long, help = "Skip sync after copy, even if requested by the profile")]
    pub no_sync: bool,
}

#[derive(Args, Clone)]
pub struct FirmwareDevicesArgs {
    #[arg(long, value_name = "FILE", help = "Firmware manifest (TOML)")]
    pub manifest: PathBuf,
    #[arg(
        long,
        value_name = "KEYBOARD",
        help = "Keyboard id declared in the manifest"
    )]
    pub keyboard: String,
    #[arg(
        long,
        value_name = "QUERY",
        help = "Override the hardware.flash device_query (default uses profile)"
    )]
    pub query: Option<String>,
    #[arg(long, help = "Ignore the profile query and list every detected device")]
    pub all: bool,
}

#[derive(Copy, Clone, ValueEnum)]
pub enum ConflictFlag {
    Prompt,
    Override,
    Skip,
    Script,
}

impl From<ConflictFlag> for ConflictPolicy {
    fn from(value: ConflictFlag) -> Self {
        match value {
            ConflictFlag::Prompt => ConflictPolicy::Prompt,
            ConflictFlag::Override => ConflictPolicy::Override,
            ConflictFlag::Skip => ConflictPolicy::Skip,
            ConflictFlag::Script => ConflictPolicy::Script,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub enum FlashSideFlag {
    Left,
    Right,
    Both,
}

impl From<FlashSideFlag> for FlashSideSelection {
    fn from(flag: FlashSideFlag) -> Self {
        match flag {
            FlashSideFlag::Left => FlashSideSelection::Left,
            FlashSideFlag::Right => FlashSideSelection::Right,
            FlashSideFlag::Both => FlashSideSelection::Both,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub enum TemplateModeFlag {
    Strip,
    Full,
}

impl From<TemplateModeFlag> for TemplateParseMode {
    fn from(flag: TemplateModeFlag) -> Self {
        match flag {
            TemplateModeFlag::Strip => TemplateParseMode::StripPlaceholders,
            TemplateModeFlag::Full => TemplateParseMode::FullDocument,
        }
    }
}
