mod firmware;
mod keymap;
mod profiles;
mod script;
mod tasks;

use crate::cli::{
    app::{Command, FirmwareCommand, KeymapCommand, ProfilesCommand},
    error::CliError,
};

use super::app::Cli;

pub fn dispatch(cli: Cli) -> Result<i32, CliError> {
    match cli.command {
        Command::Lua(args) => script::run(&args),
        Command::Firmware(cmd) => run_firmware(cmd),
        Command::Profiles(cmd) => run_profiles(cmd),
        Command::Keymap(cmd) => run_keymap(cmd),
    }
}

fn run_firmware(command: FirmwareCommand) -> Result<i32, CliError> {
    match command {
        FirmwareCommand::Build(args) => firmware::build(&args),
        FirmwareCommand::Flash(args) => firmware::flash(&args),
        FirmwareCommand::Devices(args) => firmware::devices(&args),
    }
}

fn run_profiles(command: ProfilesCommand) -> Result<i32, CliError> {
    match command {
        ProfilesCommand::Check(args) => profiles::check(&args),
    }
}

fn run_keymap(command: KeymapCommand) -> Result<i32, CliError> {
    match command {
        KeymapCommand::Apply(args) => tasks::apply(&args),
        KeymapCommand::Validate(args) => tasks::validate(&args),
        KeymapCommand::Diff(args) => tasks::diff(&args),
        KeymapCommand::Lua(args) => script::run(&args),
        KeymapCommand::Convert(args) => keymap::convert(&args),
    }
}
