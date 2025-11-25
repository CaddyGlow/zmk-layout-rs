use crate::{
    cli::{
        app::{ApplyArgs, DiffArgs, ValidateArgs},
        context::{PreparedContext, execute, prepare, print_combo_conditions, print_results},
        error::CliError,
    },
    io,
    tasks::ExecutionMode,
};

pub fn apply(args: &ApplyArgs) -> Result<i32, CliError> {
    let PreparedContext {
        file,
        document,
        layout,
    } = prepare(&args.shared)?;
    let exec = execute(document, &file, ExecutionMode::Apply);
    let code = print_results(&exec.results);
    if args.shared.combo_conditions {
        print_combo_conditions(&file);
    }
    if code != 0 {
        return Ok(code);
    }

    let (base_text, _) = layout.raw_for_diff();
    let output =
        io::serialize_keymap_with_base(exec.document, &base_text, &layout.diff_base_path())?;
    if let Some(path) = &args.output {
        io::write_text(path, &output)?;
        eprintln!("wrote updated layout to {}", path.display());
    } else {
        print!("{}", output);
    }

    Ok(code)
}

pub fn validate(args: &ValidateArgs) -> Result<i32, CliError> {
    let PreparedContext {
        file,
        document,
        layout: _,
    } = prepare(&args.shared)?;
    let exec = execute(document, &file, ExecutionMode::DryRun);
    let code = print_results(&exec.results);
    if args.shared.combo_conditions {
        print_combo_conditions(&file);
    }
    Ok(code)
}

pub fn diff(args: &DiffArgs) -> Result<i32, CliError> {
    let PreparedContext {
        file,
        document,
        layout,
    } = prepare(&args.shared)?;
    let exec = execute(document, &file, ExecutionMode::Apply);
    let code = print_results(&exec.results);
    if args.shared.combo_conditions {
        print_combo_conditions(&file);
    }
    if code != 0 {
        return Ok(code);
    }
    let (base_text, is_preprocessed) = layout.raw_for_diff();
    let updated =
        io::serialize_keymap_with_base(exec.document, &base_text, &layout.diff_base_path())?;
    if is_preprocessed {
        eprintln!("warning: diff is against preprocessed layout content");
    }
    let diff = io::render_diff(&base_text, &updated, &layout.diff_base_path());
    print!("{diff}");
    Ok(0)
}
