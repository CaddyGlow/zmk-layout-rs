//! Key position name support for Python API.

use std::cell::RefCell;
use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::PyList;

use zmk_layout_core::key_positions::KeyPositionMap;
use zmk_layout_core::profiles::KeyboardProfileDoc;

use super::util::script_error;

/// Thread-safe shared position map.
pub type SharedPositions = Arc<RefCell<KeyPositionMap>>;

/// Python wrapper for KeyPositionMap providing key position name lookups.
///
/// Allows using semantic position names like "LH_C6R1" instead of numeric indices.
#[pyclass(unsendable)]
#[derive(Clone)]
pub struct PositionMap {
    inner: SharedPositions,
}

impl PositionMap {
    pub fn new(map: KeyPositionMap) -> Self {
        Self {
            inner: Arc::new(RefCell::new(map)),
        }
    }

    pub fn shared(&self) -> SharedPositions {
        Arc::clone(&self.inner)
    }
}

#[pymethods]
impl PositionMap {
    /// Get the numeric index for a position name.
    ///
    /// Args:
    ///     name: Position name like "LH_C6R1" or "POS_LH_C6R1".
    ///
    /// Returns:
    ///     The 0-based numeric index, or None if not found.
    fn get(&self, name: &str) -> Option<u32> {
        self.inner.borrow().get(name)
    }

    /// Get the position name for a numeric index.
    ///
    /// Args:
    ///     index: The 0-based numeric index.
    ///
    /// Returns:
    ///     The position name without "POS_" prefix, or None if not found.
    fn get_name(&self, index: u32) -> Option<String> {
        self.inner.borrow().get_name(index).map(String::from)
    }

    /// Check if a position name exists in the map.
    ///
    /// Args:
    ///     name: Position name to check.
    ///
    /// Returns:
    ///     True if the name exists.
    fn contains(&self, name: &str) -> bool {
        self.inner.borrow().contains(name)
    }

    /// Get the number of positions in the map.
    ///
    /// Returns:
    ///     Total number of position mappings.
    fn __len__(&self) -> usize {
        self.inner.borrow().len()
    }

    /// Get all position names sorted alphabetically.
    ///
    /// Returns:
    ///     List of all position names.
    fn names(&self) -> Vec<String> {
        self.inner
            .borrow()
            .names()
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Resolve a mixed list of positions (names or numbers) to numeric indices.
    ///
    /// Each element can be:
    /// - A position name: "LH_C6R1", "POS_LH_T1"
    /// - A numeric value: 52, "52"
    ///
    /// Args:
    ///     positions: List of position names or numbers.
    ///
    /// Returns:
    ///     List of 0-based numeric indices.
    ///
    /// Raises:
    ///     RuntimeError: If a position name is not found.
    fn resolve(&self, positions: &Bound<'_, PyList>) -> PyResult<Vec<u32>> {
        let map = self.inner.borrow();
        let mut result = Vec::new();

        for item in positions.iter() {
            let index = resolve_single_position(&map, &item)?;
            result.push(index);
        }

        Ok(result)
    }
}

/// Load a PositionMap from a keyboard profile name.
///
/// Args:
///     profile_name: Name of the keyboard profile (e.g., "glove80").
///
/// Returns:
///     A PositionMap with all position names from the profile.
#[pyfunction]
pub fn load_positions(profile_name: &str) -> PyResult<PositionMap> {
    let profile = KeyboardProfileDoc::load(profile_name)
        .map_err(|err| script_error(format!("failed to load profile '{}': {}", profile_name, err)))?;
    let map = KeyPositionMap::from_profile(&profile);
    Ok(PositionMap::new(map))
}

/// Resolve a single Python value to a position index.
///
/// Accepts:
/// - Integer: Used directly (0-based)
/// - String that parses as integer: Used directly
/// - String position name: Looked up in the map
pub fn resolve_single_position(map: &KeyPositionMap, value: &Bound<'_, PyAny>) -> PyResult<u32> {
    // Try as integer first
    if let Ok(num) = value.extract::<i64>() {
        if num < 0 {
            return Err(script_error("position index cannot be negative"));
        }
        return Ok(num as u32);
    }

    // Try as string
    if let Ok(s) = value.extract::<String>() {
        // Try parsing as number
        if let Ok(num) = s.parse::<u32>() {
            return Ok(num);
        }

        // Try as position name
        if let Some(index) = map.get(&s) {
            return Ok(index);
        }

        return Err(script_error(format!("unknown position: {}", s)));
    }

    Err(script_error(format!(
        "position must be integer or string, got {}",
        value.get_type().name()?
    )))
}

/// Resolve a list of positions, using 1-based indexing for pure numeric inputs.
///
/// This is for backwards compatibility with the existing API which uses 1-based indices.
/// When position names are used, no adjustment is made (they map directly to 0-based).
pub fn resolve_positions_1based(
    map: &KeyPositionMap,
    positions: &Bound<'_, PyList>,
) -> PyResult<Vec<u32>> {
    let mut result = Vec::new();

    for item in positions.iter() {
        // Check if it's a pure integer (1-based)
        if let Ok(num) = item.extract::<i64>() {
            if num < 1 {
                return Err(script_error("positions must be >= 1 (1-based indexing)"));
            }
            result.push((num - 1) as u32);
            continue;
        }

        // Check if it's a string
        if let Ok(s) = item.extract::<String>() {
            // Try parsing as number (1-based)
            if let Ok(num) = s.parse::<i64>() {
                if num < 1 {
                    return Err(script_error("positions must be >= 1 (1-based indexing)"));
                }
                result.push((num - 1) as u32);
                continue;
            }

            // Try as position name (maps directly to 0-based)
            if let Some(index) = map.get(&s) {
                result.push(index);
                continue;
            }

            return Err(script_error(format!("unknown position: {}", s)));
        }

        return Err(script_error(format!(
            "position must be integer or string, got {}",
            item.get_type().name()?
        )));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_glove80_positions() {
        let map = load_positions("glove80").unwrap();
        assert_eq!(map.get("LH_C6R1"), Some(0));
        assert_eq!(map.get("LH_T1"), Some(52));
    }
}
