#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;
use crate::{
    adapters::{
        export_standard_str, export_standard_str_with_template, import_standard_str_with_template,
        moergo::export_standard_str_from_moergo_dtsi, render_standard_template,
        template_contains_placeholders,
    },
    cli::{
        app::{KeymapToDtsArgs, KeymapToJsonArgs, VendorExtractionFlag},
        error::CliError,
    },
    dts::DtsDocument,
    io,
};

pub fn to_json(args: &KeymapToJsonArgs) -> Result<i32, CliError> {
    #[cfg(feature = "ancpp-preprocessor")]
    let source = if args.preprocess.preprocess {
        let cfg = build_config(&args.preprocess, &args.dts)?;
        let handle = io::load_layout_preprocessed(&args.dts, &cfg)?;
        handle
            .preprocessed_text
            .clone()
            .or(handle.raw_text.clone())
            .unwrap_or_default()
    } else {
        io::read_text(&args.dts)?
    };
    #[cfg(not(feature = "ancpp-preprocessor"))]
    let source = io::read_text(&args.dts)?;

    if let Some(vendor) = args.vendor {
        let contents = match vendor {
            VendorExtractionFlag::Moergo => export_standard_str_from_moergo_dtsi(&source)?,
        };
        io::write_text(&args.json, &contents)?;
    } else {
        let contents = if let Some(template_path) = &args.template {
            let template_source = io::read_text(template_path)?;
            if template_contains_placeholders(&template_source) {
                export_standard_str_with_template(&source, &template_source)?
            } else {
                let document =
                    DtsDocument::parse_str(&source).map_err(|source| CliError::ParseLayout {
                        path: args.dts.clone(),
                        source,
                    })?;
                export_standard_str(&document)?
            }
        } else {
            let document =
                DtsDocument::parse_str(&source).map_err(|source| CliError::ParseLayout {
                    path: args.dts.clone(),
                    source,
                })?;
            export_standard_str(&document)?
        };
        io::write_text(&args.json, &contents)?;
    }

    eprintln!("wrote keymap JSON to {}", args.json.display());
    Ok(0)
}

pub fn to_dts(args: &KeymapToDtsArgs) -> Result<i32, CliError> {
    let json_text = io::read_text(&args.json)?;
    let template_source = io::read_text(&args.template)?;

    if template_contains_placeholders(&template_source) {
        let rendered = render_standard_template(&json_text, &template_source)?;
        io::write_text(&args.output, &rendered)?;
    } else {
        let imported = import_standard_str_with_template(&json_text, &template_source)?;
        let rendered = imported.to_string().map_err(CliError::Serialize)?;
        io::write_text(&args.output, &rendered)?;
    }

    eprintln!("rendered DTS to {}", args.output.display());
    Ok(0)
}
