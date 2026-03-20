use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{fmt, path::PathBuf};

use zmk_layout_core::{flash::FlashSideSelection, tasks::ConflictPolicy};

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
    #[command(subcommand)]
    Keymap(KeymapCommand),
    Lua(BasicScriptArgs),
    #[command(subcommand)]
    Firmware(FirmwareCommand),
    #[command(subcommand)]
    Profiles(ProfilesCommand),
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
pub struct BasicScriptArgs {
    #[arg(value_name = "SCRIPT", help = "Lua script file to execute")]
    pub script: PathBuf,
    #[arg(trailing_var_arg = true, value_name = "ARGS", help = "Arguments passed to the script")]
    pub args: Vec<String>,
}

#[derive(Args, Clone)]
pub struct ScriptArgs {
    #[arg(long, value_name = "FILE", help = "Lua script file to execute")]
    pub script: PathBuf,
    #[arg(
        long = "layout",
        value_name = "FILE",
        help = "Layout file to transform (optional; starts from a minimal layout when omitted)"
    )]
    pub layout: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        default_value_t = KeymapFormat::Dts,
        help = "Input format (json/dts/dtsi/moergo-json)"
    )]
    pub format: KeymapFormat,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Keyboard profile to use (enables vendor-specific extraction)"
    )]
    pub profile: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "Force vendor-specific regex extraction when reading Devicetree"
    )]
    pub vendor: Option<VendorExtractionFlag>,
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
    Show(ProfileShowArgs),
}

#[derive(Subcommand)]
pub enum KeymapCommand {
    Apply(ApplyArgs),
    Validate(ValidateArgs),
    Diff(DiffArgs),
    Show(KeymapShowArgs),
    #[command(name = "convert")]
    Convert(KeymapConvertArgs),
    #[command(name = "lua")]
    Lua(ScriptArgs),
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
pub struct ProfileShowArgs {
    #[arg(value_name = "PROFILE", help = "Keyboard profile name (e.g., glove80)")]
    pub profile: String,
    #[arg(long, help = "Display position names instead of numeric indices")]
    pub names: bool,
}

#[derive(Args, Clone)]
pub struct KeymapConvertArgs {
    #[arg(
        long,
        value_name = "FILE",
        help = "Input keymap file (JSON or Devicetree source)"
    )]
    pub input: PathBuf,
    #[arg(
        long,
        value_name = "FILE",
        help = "Destination keymap file (JSON or Devicetree source)"
    )]
    pub output: PathBuf,
    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        help = "Input format (json/dts/dtsi)"
    )]
    pub from: KeymapFormat,
    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        help = "Output format (json/dts/dtsi)"
    )]
    pub to: KeymapFormat,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Keyboard profile to guide conversion (template lookup, vendor extraction)"
    )]
    pub profile: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "Force vendor-specific regex extraction when reading Devicetree"
    )]
    pub vendor: Option<VendorExtractionFlag>,
    #[arg(
        long,
        value_name = "FILE",
        help = "DTS template that provides macros, includes, etc. (required when converting to dts/dtsi without --profile)"
    )]
    pub template: Option<PathBuf>,
    #[cfg(feature = "ancpp-preprocessor")]
    #[command(flatten)]
    pub preprocess: PreprocessorArgs,
}

#[derive(Args, Clone)]
pub struct KeymapShowArgs {
    #[arg(
        long,
        value_name = "FILE",
        help = "Layout file to inspect (JSON or Devicetree source)"
    )]
    pub layout: PathBuf,
    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        default_value_t = KeymapFormat::Dts,
        help = "Input format (json/dts/dtsi/moergo-json)"
    )]
    pub format: KeymapFormat,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Keyboard profile to use for formatting"
    )]
    pub profile: Option<String>,
    #[arg(
        long,
        value_name = "LAYER",
        help = "Layer to render; omit to only list layers"
    )]
    pub layer: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "Force vendor-specific regex extraction when reading Devicetree"
    )]
    pub vendor: Option<VendorExtractionFlag>,
    #[cfg(feature = "ancpp-preprocessor")]
    #[command(flatten)]
    pub preprocess: PreprocessorArgs,
}

impl KeymapConvertArgs {
    pub fn validate(&self) -> Result<(), String> {
        if self.vendor.is_some() && !self.from.is_dts_like() {
            return Err("--vendor is only supported for Devicetree input formats".into());
        }
        if self.to.is_dts_like() && self.template.is_none() && self.profile.is_none() {
            return Err("provide --template or --profile when converting to dts/dtsi".into());
        }
        Ok(())
    }
}

impl KeymapShowArgs {
    pub fn validate(&self) -> Result<(), String> {
        if self.vendor.is_some() && !self.format.is_dts_like() {
            return Err("--vendor is only supported for Devicetree input formats".into());
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum VendorExtractionFlag {
    Moergo,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum KeymapFormat {
    Json,
    Dts,
    Dtsi,
    #[value(name = "moergo-json")]
    MoergoJson,
}

impl KeymapFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeymapFormat::Json => "json",
            KeymapFormat::Dts => "dts",
            KeymapFormat::Dtsi => "dtsi",
            KeymapFormat::MoergoJson => "moergo-json",
        }
    }

    pub fn is_dts_like(&self) -> bool {
        matches!(self, KeymapFormat::Dts | KeymapFormat::Dtsi)
    }
}

impl fmt::Display for KeymapFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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
        long = "layout",
        value_name = "PATH",
        help = "Layout input (JSON, MoErgo JSON, DTS, DTSI, or keymap)"
    )]
    pub layout: Option<PathBuf>,

    #[arg(
        long = "format",
        value_enum,
        value_name = "FORMAT",
        help = "Layout format; auto-detected from extension if omitted"
    )]
    pub format: Option<KeymapFormat>,

    #[arg(
        long = "kconfig",
        value_name = "FILE",
        help = "Kconfig overlay (.conf); skips generation and appends -D defs"
    )]
    pub kconfig: Option<PathBuf>,
    #[arg(
        short = 'D',
        long = "kconfig-def",
        value_name = "NAME=VALUE",
        help = "Override or append Kconfig options (repeatable)"
    )]
    pub kconfig_defs: Vec<String>,
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
    #[arg(
        long,
        value_enum,
        value_name = "MODE",
        help = "Device detection mode: poll (default) or events"
    )]
    pub detect: Option<DetectModeFlag>,
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
pub enum DetectModeFlag {
    Poll,
    Events,
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
