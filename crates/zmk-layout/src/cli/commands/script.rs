use std::path::PathBuf;

#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;
use crate::cli::{
    app::{BasicScriptArgs, KeymapFormat, ScriptArgs, VendorExtractionFlag},
    error::CliError,
};
use zmk_layout_core::{
    adapters::{
        moergo::{export_moergo_json, export_standard_str_from_moergo_dtsi, import_moergo_json},
        standard::AdapterLayout,
    },
    dts::DtsDocument,
    io,
    keymap::KeymapDocument,
    layout_handle::{LayoutHandle, LayoutOrigin},
    profiles::KeyboardProfileDoc,
};
use zmk_layout_lua::lua_engine::{execute_script, execute_script_with_args};

/// Run the top-level `lua` command with positional script and arguments.
pub fn run_basic(args: &BasicScriptArgs) -> Result<i32, CliError> {
    let script_text = io::read_text(&args.script)?;
    let document = default_keymap_document();

    let script_dir = args.script.parent().map(|p| {
        if p.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            p.to_path_buf()
        }
    });

    let result = execute_script_with_args(document, &script_text, script_dir.as_deref(), &args.args)?;

    for log in &result.logs {
        eprintln!("{}", log);
    }

    if let Some(error) = result.error {
        eprintln!("Script execution failed: {}", error);
        return Ok(2);
    }

    Ok(0)
}

/// Run the `keymap lua` command with format-aware layout loading.
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

    let output = serialize_output(&result.document, &layout, args.format)?;

    if args.show_diff {
        let (base_text, is_preprocessed) = layout.raw_for_diff();
        if is_preprocessed {
            eprintln!("warning: diff is against preprocessed layout content");
        }
        let base_path = layout.diff_base_path();
        let diff = io::render_diff(&base_text, &output, &base_path);
        print!("{diff}");
    } else if let Some(path) = &args.output {
        io::write_text(path, &output)?;
        eprintln!("wrote updated layout to {}", path.display());
    } else {
        print!("{}", output);
    }

    Ok(0)
}

fn serialize_output(
    document: &KeymapDocument,
    layout: &io::LoadedLayout,
    format: KeymapFormat,
) -> Result<String, CliError> {
    match format {
        KeymapFormat::Json => {
            let adapter: AdapterLayout = document.clone().into();
            adapter.to_standard_json().map_err(|err| {
                CliError::InvalidArgument(format!("failed to serialize to JSON: {err}"))
            })
        }
        KeymapFormat::MoergoJson => {
            export_moergo_json(document).map_err(CliError::Adapter)
        }
        KeymapFormat::Dts | KeymapFormat::Dtsi => {
            let (base_text, _) = layout.raw_for_diff();
            let base_path = layout.diff_base_path();
            io::serialize_keymap_with_base(document.clone(), &base_text, &base_path)
                .map_err(Into::into)
        }
    }
}

fn default_keymap_document() -> KeymapDocument {
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
        .map(|h| h.as_keymap_document())
        .expect("default layout should always parse")
}

fn load_layout_or_default(args: &ScriptArgs) -> Result<io::LoadedLayout, CliError> {
    if let Some(path) = args.layout.as_ref() {
        return match args.format {
            KeymapFormat::Json => load_standard_json(path),
            KeymapFormat::MoergoJson => load_moergo_json(path),
            KeymapFormat::Dts | KeymapFormat::Dtsi => {
                load_dts_layout(path, args.profile.as_deref(), args.vendor, args)
            }
        };
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

fn load_standard_json(path: &PathBuf) -> Result<io::LoadedLayout, CliError> {
    LayoutHandle::from_json_path(path, LayoutOrigin::JsonFile).map_err(|err| {
        CliError::InvalidArgument(format!("failed to load standard JSON: {err}"))
    })
}

fn load_moergo_json(path: &PathBuf) -> Result<io::LoadedLayout, CliError> {
    let text = io::read_text(path)?;
    let keymap = import_moergo_json(&text).map_err(CliError::Adapter)?;
    Ok(LayoutHandle {
        source_path: Some(path.clone()),
        raw_text: Some(text),
        preprocessed_text: None,
        keymap,
        profile: None,
        origin: LayoutOrigin::JsonFile,
    })
}

fn load_dts_layout(
    path: &PathBuf,
    profile: Option<&str>,
    vendor: Option<VendorExtractionFlag>,
    #[allow(unused_variables)] args: &ScriptArgs,
) -> Result<io::LoadedLayout, CliError> {
    #[cfg(feature = "ancpp-preprocessor")]
    let source = if args.preprocess.preprocess {
        let cfg = build_config(&args.preprocess, path)?;
        let handle = io::load_layout_preprocessed(path, &cfg)?;
        return Ok(handle);
    } else {
        io::read_text(path)?
    };

    #[cfg(not(feature = "ancpp-preprocessor"))]
    let source = io::read_text(path)?;

    let loaded_profile = load_or_detect_profile(profile, &source)?;

    let use_moergo_extraction = match vendor {
        Some(VendorExtractionFlag::Moergo) => true,
        None => loaded_profile
            .as_ref()
            .map(|p| p.metadata.vendor.eq_ignore_ascii_case("moergo"))
            .unwrap_or(false),
    };

    let keymap = if use_moergo_extraction {
        let json = export_standard_str_from_moergo_dtsi(&source)?;
        let adapter = AdapterLayout::from_standard_json(&json).map_err(|err| {
            CliError::InvalidArgument(format!("failed to parse exported JSON: {err}"))
        })?;
        KeymapDocument::from(adapter)
    } else {
        let document = DtsDocument::parse_str(&source).map_err(|source| CliError::ParseLayout {
            path: path.clone(),
            source,
        })?;
        let adapter = AdapterLayout::from_document(&document);
        KeymapDocument::from(adapter)
    };

    Ok(LayoutHandle {
        source_path: Some(path.clone()),
        raw_text: Some(source),
        preprocessed_text: None,
        keymap,
        profile: loaded_profile,
        origin: LayoutOrigin::DtsFile,
    })
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
