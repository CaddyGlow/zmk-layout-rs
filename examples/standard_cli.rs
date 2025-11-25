use std::{fs, path::PathBuf};

use clap::{Parser, Subcommand};
use zmk_layout_rs::adapters::{
    AdapterError, export_standard_file, import_standard_str_with_template,
    render_standard_template, template_contains_placeholders,
};
use zmk_layout_rs::dts::{DtsDocument, DtsError};

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
        Command::Export {
            dts,
            json,
        } => {
            let source = fs::read_to_string(&dts)?;
            let document = DtsDocument::parse_str(&source).map_err(DtsError::from)?;
            export_standard_file(&document, json)?;
        }
        Command::Import {
            json,
            template,
            output,
        } => {
            let json_text = fs::read_to_string(&json)?;
            let template_source = fs::read_to_string(&template)?;
            if template_contains_placeholders(&template_source) {
                let rendered = render_standard_template(&json_text, &template_source)?;
                fs::write(output, rendered)?;
            } else {
                let imported = import_standard_str_with_template(&json_text, &template_source)?;
                imported.write_to_file(output)?;
            }
        }
    }
    Ok(())
}
