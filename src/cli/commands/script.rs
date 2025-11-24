use std::path::PathBuf;

#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;
use crate::{
    cli::{app::ScriptArgs, error::CliError},
    dts::DtsDocument,
    io,
    providers::KeymapDocument,
    tasks::execute_script,
};

pub fn run(args: &ScriptArgs) -> Result<i32, CliError> {
    let script_text = io::read_text(&args.script)?;
    let layout = load_layout_or_default(args)?;
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

fn load_layout_or_default(args: &ScriptArgs) -> Result<io::LoadedLayout, CliError> {
    if let Some(path) = args.layout.as_ref() {
        #[cfg(feature = "ancpp-preprocessor")]
        {
            if args.preprocess.preprocess {
                let cfg = build_config(&args.preprocess, path)?;
                return Ok(io::load_layout_preprocessed(path, &cfg)?);
            }
        }
        return Ok(io::load_layout(path)?);
    }

    #[cfg(feature = "ancpp-preprocessor")]
    {
        if args.preprocess.preprocess {
            return Err(CliError::InvalidArgument(
                "--preprocess requires a layout path".into(),
            ));
        }
    }

    let source = r#"
/ {
    behaviors {};
    macros {};
    combos {};
};

keymap {
    compatible = "zmk,keymap";
    base {
        bindings = < &none >;
    };
};
"#;
    let document = DtsDocument::parse_str(source).map_err(|err| CliError::ParseLayout {
        path: PathBuf::from("<memory>"),
        source: err,
    })?;
    let text = document.to_string().map_err(CliError::Serialize)?;
    Ok(io::LoadedLayout {
        path: PathBuf::from("<memory>"),
        text,
        document,
    })
}
