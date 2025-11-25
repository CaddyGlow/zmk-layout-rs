#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;
use crate::{
    adapters::{
        export_standard_str, import_standard_str_with_template, moergo::export_standard_str_from_moergo_dtsi,
        render_standard_template, template_contains_placeholders,
    },
    cli::{
        app::{KeymapToDtsArgs, KeymapToJsonArgs, VendorExtractionFlag},
        error::CliError,
    },
    dts::DtsDocument,
    io,
    profiles::KeyboardProfileDoc,
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

    let profile = if let Some(profile) = &args.profile {
        Some(
            KeyboardProfileDoc::load(profile)
                .map_err(|err| CliError::InvalidArgument(format!("failed to load profile {profile}: {err}")))?,
        )
    } else {
        let matches = KeyboardProfileDoc::detect_from_rendered(&source);
        if matches.len() > 1 {
            return Err(CliError::InvalidArgument(
                "multiple profiles matched the input; specify --profile to disambiguate".into(),
            ));
        }
        matches.into_iter().next()
    };

    let use_moergo_extractions = match args.vendor {
        Some(VendorExtractionFlag::Moergo) => true,
        None => profile
            .as_ref()
            .map(|p| p.metadata.vendor.eq_ignore_ascii_case("moergo"))
            .unwrap_or(false),
    };

    let contents = if use_moergo_extractions {
        export_standard_str_from_moergo_dtsi(&source)?
    } else {
        let document = DtsDocument::parse_str(&source).map_err(|source| CliError::ParseLayout {
            path: args.dts.clone(),
            source,
        })?;
        export_standard_str(&document)?
    };
    io::write_text(&args.json, &contents)?;

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
