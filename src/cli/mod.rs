pub mod app;
mod commands;
pub mod context;
pub mod error;

pub use app::Cli;
pub use error::CliError;

pub fn run() -> Result<i32, CliError> {
    let cli = app::Cli::parse();
    commands::dispatch(cli)
}
