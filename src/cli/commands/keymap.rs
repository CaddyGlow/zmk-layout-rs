#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;
use crate::{
    adapters::{
        export_standard_str, import_standard_str_with_template, moergo,
        moergo::export_standard_str_from_moergo_dtsi, render_standard_template,
        standard::AdapterLayout, template_contains_placeholders,
    },
    cli::{
        app::{KeymapConvertArgs, KeymapFormat, VendorExtractionFlag},
        error::CliError,
    },
    dts::DtsDocument,
    io,
    keymap::KeymapDocument,
    profiles::KeyboardProfileDoc,
};
use std::path::PathBuf;

pub fn convert(args: &KeymapConvertArgs) -> Result<i32, CliError> {
    args.validate().map_err(CliError::InvalidArgument)?;
    let keymap = load_keymap(args)?;
    let output_text = match args.to {
        KeymapFormat::Json => keymap_to_standard_json(&keymap)?,
        KeymapFormat::MoergoJson => moergo::export_moergo_json(&keymap)?,
        KeymapFormat::Dts | KeymapFormat::Dtsi => keymap_to_dts(&keymap, args)?,
    };

    io::write_text(&args.output, &output_text)?;
    eprintln!("wrote {} to {}", args.to.as_str(), args.output.display());
    Ok(0)
}

fn load_keymap(args: &KeymapConvertArgs) -> Result<KeymapDocument, CliError> {
    match args.from {
        KeymapFormat::Json => load_standard_json(&args.input),
        KeymapFormat::MoergoJson => load_moergo_json(&args.input),
        KeymapFormat::Dts | KeymapFormat::Dtsi => load_dts_as_keymap(args),
    }
}

fn load_standard_json(path: &PathBuf) -> Result<KeymapDocument, CliError> {
    let text = io::read_text(path)?;
    let adapter = AdapterLayout::from_standard_json(&text).map_err(|err| {
        CliError::InvalidArgument(format!("failed to parse standard JSON: {err}"))
    })?;
    Ok(KeymapDocument::from(adapter))
}

fn load_moergo_json(path: &PathBuf) -> Result<KeymapDocument, CliError> {
    let text = io::read_text(path)?;
    moergo::import_moergo_json(&text).map_err(CliError::Adapter)
}

fn load_dts_as_keymap(args: &KeymapConvertArgs) -> Result<KeymapDocument, CliError> {
    #[cfg(feature = "ancpp-preprocessor")]
    let source = if args.preprocess.preprocess {
        let cfg = build_config(&args.preprocess, &args.input)?;
        let handle = io::load_layout_preprocessed(&args.input, &cfg)?;
        handle
            .preprocessed_text
            .clone()
            .or(handle.raw_text.clone())
            .unwrap_or_default()
    } else {
        io::read_text(&args.input)?
    };
    #[cfg(not(feature = "ancpp-preprocessor"))]
    let source = io::read_text(&args.input)?;

    let profile = load_or_detect_profile(args.profile.as_deref(), &source)?;

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
            path: args.input.clone(),
            source,
        })?;
        export_standard_str(&document)?
    };
    let adapter = AdapterLayout::from_standard_json(&contents).map_err(|err| {
        CliError::InvalidArgument(format!("failed to parse exported JSON: {err}"))
    })?;
    Ok(KeymapDocument::from(adapter))
}

fn keymap_to_standard_json(document: &KeymapDocument) -> Result<String, CliError> {
    let adapter: AdapterLayout = document.clone().into();
    adapter.to_standard_json().map_err(|err| {
        CliError::InvalidArgument(format!("failed to serialize keymap to JSON: {err}"))
    })
}

fn keymap_to_dts(keymap: &KeymapDocument, args: &KeymapConvertArgs) -> Result<String, CliError> {
    let adapter: AdapterLayout = keymap.clone().into();
    let json_text = adapter.to_standard_json().map_err(|err| {
        CliError::InvalidArgument(format!("failed to serialize keymap to JSON: {err}"))
    })?;
    let template_source = resolve_template_source(args)?;

    if template_contains_placeholders(&template_source) {
        let rendered = render_standard_template(&json_text, &template_source)?;
        Ok(rendered)
    } else {
        let imported = import_standard_str_with_template(&json_text, &template_source)?;
        let rendered = imported.to_string().map_err(CliError::Serialize)?;
        Ok(rendered)
    }
}

fn resolve_template_source(args: &KeymapConvertArgs) -> Result<String, CliError> {
    if let Some(template) = &args.template {
        return Ok(io::read_text(template)?);
    }

    if let Some(profile) = &args.profile {
        let profile = KeyboardProfileDoc::load(profile).map_err(|err| {
            CliError::InvalidArgument(format!("failed to load profile {profile}: {err}"))
        })?;
        let path = resolve_profile_template_path(&profile.layout.template);
        return Ok(io::read_text(path)?);
    }

    Err(CliError::InvalidArgument(
        "provide --template or --profile when converting to dts/dtsi".into(),
    ))
}

fn load_or_detect_profile(
    profile_name: Option<&str>,
    source: &str,
) -> Result<Option<KeyboardProfileDoc>, CliError> {
    if let Some(profile) = profile_name {
        return KeyboardProfileDoc::load(profile).map(Some).map_err(|err| {
            CliError::InvalidArgument(format!("failed to load profile {profile}: {err}"))
        });
    }

    let matches = KeyboardProfileDoc::detect_from_rendered(source);
    if matches.len() > 1 {
        return Err(CliError::InvalidArgument(
            "multiple profiles matched the input; specify --profile to disambiguate".into(),
        ));
    }
    Ok(matches.into_iter().next())
}

fn resolve_profile_template_path(path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate
    } else {
        PathBuf::from(".").join(candidate)
    }
}
