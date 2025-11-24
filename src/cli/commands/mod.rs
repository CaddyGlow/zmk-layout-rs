mod firmware;
mod layer;
mod profiles;
mod script;
mod tasks;

use crate::cli::{
    app::{Command, FirmwareCommand, LayerCommand, ProfilesCommand},
    error::CliError,
};

use super::app::Cli;

pub fn dispatch(cli: Cli) -> Result<i32, CliError> {
    match cli.command {
        Command::Apply(args) => tasks::apply(&args),
        Command::Validate(args) => tasks::validate(&args),
        Command::Diff(args) => tasks::diff(&args),
        Command::Script(args) => script::run(&args),
        Command::Firmware(cmd) => run_firmware(cmd),
        Command::Profiles(cmd) => run_profiles(cmd),
        Command::Layer(cmd) => run_layer(cmd),
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

fn run_layer(command: LayerCommand) -> Result<i32, CliError> {
    match command {
        LayerCommand::Export(args) => layer::export(&args),
        LayerCommand::Import(args) => layer::import(&args),
    }
}
