#[cfg(feature = "ancpp-preprocessor")]
use crate::cli::preprocess::build_config;
use crate::{
    adapters::{
        AdapterError, export_standard_str, import_standard_str_with_template,
        moergo::export_standard_str_from_moergo_dtsi, pipeline::AdapterPipeline,
        render_standard_template, template_contains_placeholders,
    },
    cli::{
        app::{LayerExportArgs, LayerImportArgs, VendorExtractionFlag},
        error::CliError,
    },
    dts::DtsDocument,
    io,
};

pub fn export(args: &LayerExportArgs) -> Result<i32, CliError> {
    #[cfg(feature = "ancpp-preprocessor")]
    let source = if args.preprocess.preprocess {
        let cfg = build_config(&args.preprocess, &args.dts)?;
        io::load_layout_preprocessed(&args.dts, &cfg)?.text
    } else {
        io::read_text(&args.dts)?
    };
    #[cfg(not(feature = "ancpp-preprocessor"))]
    let source = io::read_text(&args.dts)?;

    if let Some(vendor) = args.vendor {
        if args.template.is_some() {
            return Err(CliError::InvalidArgument(
                "choose either --vendor or --template, not both".into(),
            ));
        }
        let contents = match vendor {
            VendorExtractionFlag::Moergo => export_standard_str_from_moergo_dtsi(&source)?,
        };
        io::write_text(&args.json, &contents)?;
    } else if let Some(template_path) = &args.template {
        let template_source = io::read_text(template_path)?;
        let pipeline = AdapterPipeline::from_dts_text(source)
            .template_source(template_source)
            .template_mode(args.template_mode.into());
        let layout = pipeline.load()?;
        let json = layout
            .to_standard_json()
            .map_err(AdapterError::from)
            .map_err(CliError::from)?;
        io::write_text(&args.json, &json)?;
    } else {
        let document = DtsDocument::parse_str(&source).map_err(|source| CliError::ParseLayout {
            path: args.dts.clone(),
            source,
        })?;
        let contents = export_standard_str(&document)?;
        io::write_text(&args.json, &contents)?;
    }

    eprintln!("exported layout to {}", args.json.display());
    Ok(0)
}

pub fn import(args: &LayerImportArgs) -> Result<i32, CliError> {
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

    eprintln!("imported layout to {}", args.output.display());
    Ok(0)
}
