#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;
use crate::cli::{
    app::{KeymapConvertArgs, KeymapFormat, KeymapShowArgs, VendorExtractionFlag},
    error::CliError,
};
use std::path::PathBuf;
use zmk_layout_core::{
    adapters::{
        export_standard_str, import_standard_str_with_template, moergo,
        moergo::export_standard_str_from_moergo_dtsi, render_standard_template,
        standard::AdapterLayout, template_contains_placeholders,
    },
    dts::DtsDocument,
    formatting::render_layer_lines,
    io,
    keymap::KeymapDocument,
    profiles::KeyboardProfileDoc,
};

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

pub fn show(args: &KeymapShowArgs) -> Result<i32, CliError> {
    args.validate().map_err(CliError::InvalidArgument)?;
    let (keymap, detected_profile) = load_keymap_for_show(args)?;
    print_layer_list(&keymap);

    let Some(layer_name) = args.layer.as_deref() else {
        return Ok(0);
    };
    let layer = keymap
        .layers
        .iter()
        .find(|layer| layer.name == layer_name)
        .ok_or_else(|| {
            CliError::InvalidArgument(format!("layer `{}` not found in layout", layer_name))
        })?;

    let profile = resolve_profile_for_render(args.profile.as_deref(), detected_profile)?;
    let formatting = &profile.layout.formatting;
    let rows: Vec<Vec<i32>> = formatting.rows.iter().map(|row| row.keys.clone()).collect();
    if rows.is_empty() {
        return Err(CliError::InvalidArgument(format!(
            "profile {} is missing layout.formatting rows",
            profile.metadata.name
        )));
    }

    println!();
    println!(
        "layer `{}` (profile: {}):",
        layer.name, profile.metadata.name
    );
    let lines = render_layer_lines(
        &layer.bindings,
        &rows,
        formatting.key_gap.as_deref().unwrap_or("  "),
        formatting.base_indent.as_deref().unwrap_or(""),
    );
    for line in lines {
        println!("{line}");
    }
    Ok(0)
}

fn print_layer_list(keymap: &KeymapDocument) {
    println!("layers ({}):", keymap.layers.len());
    for layer in &keymap.layers {
        println!("- {}", layer.name);
    }
}

fn load_keymap_for_show(
    args: &KeymapShowArgs,
) -> Result<(KeymapDocument, Option<KeyboardProfileDoc>), CliError> {
    match args.format {
        KeymapFormat::Json => {
            let keymap = load_standard_json(&args.layout)?;
            let profile = load_profile_if_specified(args.profile.as_deref())?;
            Ok((keymap, profile))
        }
        KeymapFormat::MoergoJson => {
            let keymap = load_moergo_json(&args.layout)?;
            let profile = load_profile_if_specified(args.profile.as_deref())?;
            Ok((keymap, profile))
        }
        KeymapFormat::Dts | KeymapFormat::Dtsi => load_dts_keymap_with_profile(
            &args.layout,
            args.profile.as_deref(),
            args.vendor,
            #[cfg(feature = "ancpp-preprocessor")]
            Some(&args.preprocess),
        ),
    }
}

fn resolve_profile_for_render(
    requested: Option<&str>,
    detected: Option<KeyboardProfileDoc>,
) -> Result<KeyboardProfileDoc, CliError> {
    if let Some(profile) = detected {
        return Ok(profile);
    }
    let Some(name) = requested else {
        return Err(CliError::InvalidArgument(
            "profile is required to render a layer; provide --profile or use a layout that matches a profile".into(),
        ));
    };
    load_profile_if_specified(Some(name))?
        .ok_or_else(|| CliError::InvalidArgument(format!("failed to load profile {name}")))
}

fn load_profile_if_specified(name: Option<&str>) -> Result<Option<KeyboardProfileDoc>, CliError> {
    match name {
        Some(profile) => load_profile_by_name_or_path(profile).map(Some),
        None => Ok(None),
    }
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
    let (keymap, _) = load_dts_keymap_with_profile(
        &args.input,
        args.profile.as_deref(),
        args.vendor,
        #[cfg(feature = "ancpp-preprocessor")]
        Some(&args.preprocess),
    )?;
    Ok(keymap)
}

fn load_dts_keymap_with_profile(
    path: &PathBuf,
    profile: Option<&str>,
    vendor: Option<VendorExtractionFlag>,
    #[cfg(feature = "ancpp-preprocessor")] preprocess: Option<&crate::cli::app::PreprocessorArgs>,
) -> Result<(KeymapDocument, Option<KeyboardProfileDoc>), CliError> {
    #[cfg(feature = "ancpp-preprocessor")]
    let source = if preprocess.map(|args| args.preprocess).unwrap_or(false) {
        let args = preprocess.expect("preprocess args must exist when enabled");
        let cfg = build_config(args, path)?;
        let handle = io::load_layout_preprocessed(path, &cfg)?;
        handle
            .preprocessed_text
            .clone()
            .or(handle.raw_text.clone())
            .unwrap_or_default()
    } else {
        io::read_text(path)?
    };
    #[cfg(not(feature = "ancpp-preprocessor"))]
    let source = io::read_text(path)?;

    let profile = load_or_detect_profile(profile, &source)?;

    let use_moergo_extractions = match vendor {
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
            path: path.clone(),
            source,
        })?;
        export_standard_str(&document)?
    };
    let adapter = AdapterLayout::from_standard_json(&contents).map_err(|err| {
        CliError::InvalidArgument(format!("failed to parse exported JSON: {err}"))
    })?;
    Ok((KeymapDocument::from(adapter), profile))
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
        let profile = load_profile_by_name_or_path(profile)?;
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
        return load_profile_by_name_or_path(profile).map(Some);
    }

    let matches = KeyboardProfileDoc::detect_from_rendered(source);
    if matches.len() > 1 {
        return Err(CliError::InvalidArgument(
            "multiple profiles matched the input; specify --profile to disambiguate".into(),
        ));
    }
    Ok(matches.into_iter().next())
}

fn load_profile_by_name_or_path(profile: &str) -> Result<KeyboardProfileDoc, CliError> {
    let candidate = PathBuf::from(profile);
    if candidate.exists() {
        return KeyboardProfileDoc::from_file(&candidate).map_err(|err| {
            CliError::InvalidArgument(format!(
                "failed to load profile {}: {err}",
                candidate.display()
            ))
        });
    }

    KeyboardProfileDoc::load(profile).map_err(|err| {
        CliError::InvalidArgument(format!("failed to load profile {profile}: {err}"))
    })
}

fn resolve_profile_template_path(path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate
    } else {
        PathBuf::from(".").join(candidate)
    }
}
