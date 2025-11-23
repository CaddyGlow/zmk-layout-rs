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
        layout: _,
    } = prepare(&args.shared)?;
    let exec = execute(document, &file, ExecutionMode::Apply);
    let code = print_results(&exec.results);
    if args.shared.combo_conditions {
        print_combo_conditions(&file);
    }
    if code != 0 {
        return Ok(code);
    }

    let output = io::serialize_keymap(exec.document)?;
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
    let updated = io::serialize_keymap(exec.document)?;
    let diff = io::render_diff(&layout.text, &updated, &layout.path);
    print!("{diff}");
    Ok(0)
}
