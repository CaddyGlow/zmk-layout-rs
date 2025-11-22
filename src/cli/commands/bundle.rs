use crate::{
    adapters::bundle::LayoutBundle,
    cli::{
        app::{BundleExportArgs, BundleFormat, BundleImportArgs, BundleRenderArgs},
        error::CliError,
    },
    io,
};

pub fn import(args: &BundleImportArgs) -> Result<i32, CliError> {
    match args.format {
        BundleFormat::Moergo => {
            let bundle = LayoutBundle::from_moergo_file(&args.input)?;
            bundle.write_json(&args.output)?;
        }
    }
    eprintln!("imported bundle to {}", args.output.display());
    Ok(0)
}

pub fn export(args: &BundleExportArgs) -> Result<i32, CliError> {
    let bundle = LayoutBundle::from_json_file(&args.bundle)?;
    match args.format {
        BundleFormat::Moergo => {
            let json = bundle.to_moergo_json()?;
            io::write_text(&args.output, &json)?;
        }
    }
    eprintln!("exported bundle to {}", args.output.display());
    Ok(0)
}

pub fn render(args: &BundleRenderArgs) -> Result<i32, CliError> {
    let bundle = LayoutBundle::from_json_file(&args.bundle)?;
    let rendered = bundle.render_target(&args.target, args.template.as_deref())?;
    if let Some(path) = &args.output {
        io::write_text(path, &rendered)?;
        eprintln!("rendered target {} to {}", args.target, path.display());
    } else {
        println!("{rendered}");
    }
    Ok(0)
}
