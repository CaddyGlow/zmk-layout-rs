use std::path::PathBuf;

use crate::{
    cli::{app::ScriptArgs, error::CliError},
    io,
    providers::KeymapDocument,
    tasks::execute_script,
};
#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;

pub fn run(args: &ScriptArgs) -> Result<i32, CliError> {
    let script_text = io::read_text(&args.script)?;
    #[cfg(feature = "ancpp-preprocessor")]
    let layout = if args.preprocess.preprocess {
        let cfg = build_config(&args.preprocess, &args.layout)?;
        io::load_layout_preprocessed(&args.layout, &cfg)?
    } else {
        io::load_layout(&args.layout)?
    };
    #[cfg(not(feature = "ancpp-preprocessor"))]
    let layout = io::load_layout(&args.layout)?;
    let document = KeymapDocument::from_document(layout.document.clone());

    let script_dir = args.script.parent().map(|p| {
        if p.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            p.to_path_buf()
        }
    });

    let result = execute_script(document, &script_text, script_dir.as_deref())?;

    if !result.logs.is_empty() {
        for log in &result.logs {
            eprintln!("{}", log);
        }
    }

    if let Some(error) = result.error {
        eprintln!("Script execution failed: {}", error);
        return Ok(2);
    }

    let output = io::serialize_keymap(result.document)?;

    if args.show_diff {
        let diff = io::render_diff(&layout.text, &output, &layout.path);
        print!("{diff}");
    } else if let Some(path) = &args.output {
        io::write_text(path, &output)?;
        eprintln!("wrote updated layout to {}", path.display());
    } else {
        print!("{}", output);
    }

    Ok(0)
}
