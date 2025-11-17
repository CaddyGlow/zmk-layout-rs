use std::path::PathBuf;

use clap::{Parser, Subcommand};
use zmk_layout_rs::adapters::{
    AdapterError, export_standard_file, import_standard_file_with_template,
};
use zmk_layout_rs::dts::DtsDocument;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Example CLI for converting between DTS and the standard JSON layout format."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Export a DTS document to the standard JSON format.
    Export {
        /// Input DTS/.dtsi file to parse.
        #[arg(long)]
        dts: PathBuf,
        /// Destination JSON file to write.
        #[arg(long)]
        json: PathBuf,
    },
    /// Import a standard JSON layout and write a DTS document.
    Import {
        /// Standard JSON layout file.
        #[arg(long)]
        json: PathBuf,
        /// DTS template that provides macros, includes, etc.
        #[arg(long)]
        template: PathBuf,
        /// Output DTS path to write.
        #[arg(long)]
        output: PathBuf,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), AdapterError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Export { dts, json } => {
            let document = DtsDocument::parse_file(dts)?;
            export_standard_file(&document, json)?;
        }
        Command::Import {
            json,
            template,
            output,
        } => {
            let imported = import_standard_file_with_template(json, template)?;
            imported.write_to_file(output)?;
        }
    }
    Ok(())
}
