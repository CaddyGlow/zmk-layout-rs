use std::{
    collections::{BTreeMap, BTreeSet},
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
        let (discovered, embedded_fallback) = discover_profile_paths(&args.profiles_dir)?;
        from_embedded = embedded_fallback;
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
            if let Some(name) = profile_name_from_path(&path) {
                KeyboardProfileDoc::load(&name)
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
    if failures == 0 {
        Ok(0)
    } else {
        Ok(2)
    }
}

fn discover_profile_paths(dir: &Path) -> Result<(Vec<PathBuf>, bool), CliError> {
    if dir.exists() {
        let mut stack = vec![dir.to_path_buf()];
        let mut profiles: BTreeMap<String, (PathBuf, u8)> = BTreeMap::new();
        while let Some(current) = stack.pop() {
            let read_dir = fs::read_dir(&current).map_err(|err| {
                CliError::ProfileCheck(format!("failed to read {}: {}", current.display(), err))
            })?;
            for entry in read_dir {
                let entry = entry.map_err(|err| {
                    CliError::ProfileCheck(format!(
                        "failed to enumerate {}: {}",
                        current.display(),
                        err
                    ))
                })?;
                let path = entry.path();
                let file_type = entry.file_type().map_err(|err| {
                    CliError::ProfileCheck(format!("failed to inspect {}: {}", path.display(), err))
                })?;
                if file_type.is_dir() {
                    stack.push(path);
                    continue;
                }
                if !file_type.is_file() || !is_toml_file(&path) {
                    continue;
                }
                if let Some(name) = profile_name_from_path(&path) {
                    let priority = profile_path_priority(&path);
                    match profiles.get(&name) {
                        Some((_, existing_priority)) if *existing_priority <= priority => {}
                        _ => {
                            profiles.insert(name, (path, priority));
                        }
                    }
                }
            }
        }
        if !profiles.is_empty() {
            let mut paths: Vec<_> = profiles.into_iter().map(|(_, (path, _))| path).collect();
            paths.sort();
            return Ok((paths, false));
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

    Ok((
        available
            .into_iter()
            .map(|name| dir.join(name).join("profile.toml"))
            .collect(),
        true,
    ))
}

pub fn show(args: &ProfileShowArgs) -> Result<i32, CliError> {
    let profile = KeyboardProfileDoc::load(&args.profile).map_err(|err| {
        CliError::ProfileCheck(format!(
            "failed to load profile '{}': {}",
            args.profile, err
        ))
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

fn is_toml_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some(ext) if ext.eq_ignore_ascii_case("toml")
    )
}

fn profile_name_from_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    if !ext.eq_ignore_ascii_case("toml") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    if stem.eq_ignore_ascii_case("profile") {
        return path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .map(|name| name.to_string());
    }
    Some(stem.to_string())
}

fn profile_path_priority(path: &Path) -> u8 {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if filename.eq_ignore_ascii_case("profile.toml") {
        return 0;
    }
    let parent_matches = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .and_then(|parent_name| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.eq_ignore_ascii_case(parent_name))
        })
        .unwrap_or(false);
    if parent_matches {
        return 1;
    }
    2
}
