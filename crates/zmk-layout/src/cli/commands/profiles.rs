use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use crate::cli::{
    app::{ProfileCheckArgs, ProfileShowArgs},
    error::CliError,
};
use zmk_layout_core::{
    key_positions::KeyPositionMap,
    profiles::{KeyboardProfileDoc, LayoutFormattingRow},
};

pub fn check(args: &ProfileCheckArgs) -> Result<i32, CliError> {
    let mut requested = args.paths.clone();
    let mut from_embedded = false;
    if args.all {
        let discovered = discover_profile_paths(&args.profiles_dir)?;
        from_embedded = !args.profiles_dir.exists();
        requested.extend(discovered);
    }
    if requested.is_empty() {
        return Err(CliError::ProfileCheck(
            "provide at least one profile path or use --all".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut failures = 0;
    for path in requested {
        if !seen.insert(path.clone()) {
            continue;
        }

        // Try to load from file if it exists, otherwise try by name from embedded
        let result = if path.exists() {
            KeyboardProfileDoc::from_file(&path)
        } else if from_embedded {
            // Extract name from path for embedded loading
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                KeyboardProfileDoc::load(name)
            } else {
                KeyboardProfileDoc::from_file(&path)
            }
        } else {
            KeyboardProfileDoc::from_file(&path)
        };

        match result {
            Ok(profile) => {
                println!(
                    "[OK ] {} :: {} (keyboard `{}`)",
                    path.display(),
                    profile.metadata.name,
                    profile.keyboard
                );
            }
            Err(err) => {
                failures += 1;
                eprintln!("[ERR] {} :: {err}", path.display());
            }
        }
    }
    if failures == 0 { Ok(0) } else { Ok(2) }
}

fn discover_profile_paths(dir: &Path) -> Result<Vec<PathBuf>, CliError> {
    if dir.exists() {
        let read_dir = fs::read_dir(dir).map_err(|err| {
            CliError::ProfileCheck(format!("failed to read {}: {}", dir.display(), err))
        })?;
        let mut profiles = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|err| {
                CliError::ProfileCheck(format!("failed to enumerate {}: {}", dir.display(), err))
            })?;
            let path = entry.path();
            if matches!(path.extension().and_then(|ext| ext.to_str()), Some(ext) if ext.eq_ignore_ascii_case("toml"))
            {
                profiles.push(path);
            }
        }
        if !profiles.is_empty() {
            profiles.sort();
            return Ok(profiles);
        }
    }

    // Fallback to embedded profiles
    let available = KeyboardProfileDoc::list_available();
    if available.is_empty() {
        return Err(CliError::ProfileCheck(format!(
            "no profiles found in {} or embedded in binary",
            dir.display()
        )));
    }

    Ok(available
        .into_iter()
        .map(|name| dir.join(format!("{}.toml", name)))
        .collect())
}

pub fn show(args: &ProfileShowArgs) -> Result<i32, CliError> {
    let profile = KeyboardProfileDoc::load(&args.profile).map_err(|err| {
        CliError::ProfileCheck(format!("failed to load profile '{}': {}", args.profile, err))
    })?;
    let position_map = KeyPositionMap::from_profile(&profile);

    println!(
        "{} - {} keys",
        profile.metadata.name, profile.hardware.key_count
    );
    println!();

    for row in &profile.layout.formatting.rows {
        let line = if args.names {
            format_row_with_names(row, &position_map)
        } else {
            row.ascii_art()
        };
        println!("{}", line);
    }

    Ok(0)
}

fn format_row_with_names(row: &LayoutFormattingRow, positions: &KeyPositionMap) -> String {
    const EMPTY: &str = "        .";
    row.keys
        .iter()
        .map(|&value| {
            if value < 0 {
                EMPTY.to_string()
            } else {
                match positions.get_name(value as u32) {
                    Some(name) => format!("{:>9}", name),
                    None => format!("{:>9}", value),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
