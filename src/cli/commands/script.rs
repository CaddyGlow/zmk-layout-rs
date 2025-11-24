use std::path::PathBuf;

#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;
use crate::{
    cli::{app::ScriptArgs, error::CliError},
    io,
    layout_handle::{LayoutHandle, LayoutOrigin},
    tasks::execute_script,
};

pub fn run(args: &ScriptArgs) -> Result<i32, CliError> {
    let script_text = io::read_text(&args.script)?;
    let layout = load_layout_or_default(args)?;
    let document = layout.as_keymap_document();

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
        let (base_text, is_preprocessed) = layout.raw_for_diff();
        if is_preprocessed {
            eprintln!("warning: diff is against preprocessed layout content");
        }
        let diff = io::render_diff(&base_text, &output, &layout.diff_base_path());
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
    LayoutHandle::from_dts_text(source, LayoutOrigin::Generated)
        .map_err(|err| CliError::InvalidArgument(format!("failed to build default layout: {err}")))
}
