//! Key position name mapping for keyboards.
//!
//! This module provides support for using semantic key position names like `LH_C6R1`
//! instead of numeric indices when defining combos and layers.
//!
//! # Moergo/Glove80 Naming Convention
//!
//! Position names follow the pattern:
//! - `{LH|RH}_C{col}R{row}` - Main matrix keys (Left/Right Hand, Column, Row)
//! - `{LH|RH}_T{n}` - Thumb cluster keys
//!
//! Column numbering: right-to-left starting at 1
//! Row numbering: top-to-bottom starting at 1
//!
//! # Example
//!
//! ```ignore
//! use zmk_layout_core::key_positions::KeyPositionMap;
//! use zmk_layout_core::profiles::KeyboardProfileDoc;
//!
//! let profile = KeyboardProfileDoc::load("glove80").unwrap();
//! let positions = KeyPositionMap::from_profile(&profile);
//!
//! // Look up by name (without POS_ prefix)
//! assert_eq!(positions.get("LH_C6R1"), Some(0));
//! assert_eq!(positions.get("LH_T1"), Some(52));
//!
//! // Also works with POS_ prefix
//! assert_eq!(positions.get("POS_LH_C6R1"), Some(0));
//! ```

use std::collections::HashMap;

use regex::Regex;

use crate::profiles::KeyboardProfileDoc;

/// A bidirectional mapping between key position names and their numeric indices.
#[derive(Debug, Clone, Default)]
pub struct KeyPositionMap {
    /// Map from position name (without POS_ prefix) to index
    name_to_index: HashMap<String, u32>,
    /// Map from index to position name (without POS_ prefix)
    index_to_name: HashMap<u32, String>,
}

impl KeyPositionMap {
    /// Create an empty position map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a position map from a keyboard profile.
    ///
    /// Parses the `key_position_header` property which contains C preprocessor
    /// defines like `#define POS_LH_C6R1 0`.
    pub fn from_profile(profile: &KeyboardProfileDoc) -> Self {
        let header = profile.layout.keymap.key_position_header().unwrap_or("");
        Self::from_header(header)
    }

    /// Parse position definitions from a C header string.
    ///
    /// Expects lines in the format: `#define POS_NAME INDEX`
    pub fn from_header(header: &str) -> Self {
        let mut map = Self::new();
        let define_re = Regex::new(r"#define\s+POS_(\w+)\s+(\d+)").expect("valid regex");

        for line in header.lines() {
            if let Some(captures) = define_re.captures(line) {
                let name = captures.get(1).map(|m| m.as_str()).unwrap_or("");
                let index_str = captures.get(2).map(|m| m.as_str()).unwrap_or("");

                if let Ok(index) = index_str.parse::<u32>() {
                    map.insert(name.to_string(), index);
                }
            }
        }

        map
    }

    /// Insert a position name and index mapping.
    pub fn insert(&mut self, name: String, index: u32) {
        self.index_to_name.insert(index, name.clone());
        self.name_to_index.insert(name, index);
    }

    /// Look up a position index by name.
    ///
    /// Accepts names with or without the `POS_` prefix:
    /// - `"LH_C6R1"` -> Some(0)
    /// - `"POS_LH_C6R1"` -> Some(0)
    pub fn get(&self, name: &str) -> Option<u32> {
        // Strip POS_ prefix if present
        let normalized = name.strip_prefix("POS_").unwrap_or(name);
        self.name_to_index.get(normalized).copied()
    }

    /// Look up a position name by index.
    ///
    /// Returns the name without the `POS_` prefix.
    pub fn get_name(&self, index: u32) -> Option<&str> {
        self.index_to_name.get(&index).map(|s| s.as_str())
    }

    /// Check if a name exists in the map.
    pub fn contains(&self, name: &str) -> bool {
        let normalized = name.strip_prefix("POS_").unwrap_or(name);
        self.name_to_index.contains_key(normalized)
    }

    /// Get the total number of positions in the map.
    pub fn len(&self) -> usize {
        self.name_to_index.len()
    }

    /// Check if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.name_to_index.is_empty()
    }

    /// Iterate over all (name, index) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, u32)> {
        self.name_to_index
            .iter()
            .map(|(name, &index)| (name.as_str(), index))
    }

    /// Get all position names sorted alphabetically.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.name_to_index.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Resolve a mixed list of position references (names or indices) to indices.
    ///
    /// Each element can be:
    /// - A position name like "LH_C6R1" or "POS_LH_C6R1"
    /// - A numeric string like "52"
    ///
    /// Returns `Err` with the invalid position name if lookup fails.
    pub fn resolve_positions(&self, positions: &[String]) -> Result<Vec<u32>, String> {
        positions
            .iter()
            .map(|pos| self.resolve_position(pos))
            .collect()
    }

    /// Resolve a single position reference to an index.
    ///
    /// Accepts:
    /// - Position names: "LH_C6R1", "POS_LH_C6R1"
    /// - Numeric strings: "52", "0"
    pub fn resolve_position(&self, position: &str) -> Result<u32, String> {
        // Try as numeric first
        if let Ok(index) = position.parse::<u32>() {
            return Ok(index);
        }

        // Try as position name
        self.get(position)
            .ok_or_else(|| format!("unknown position: {}", position))
    }
}

/// Extension trait for types that can resolve position names.
pub trait PositionResolver {
    /// Resolve a position reference (name or index string) to an index.
    fn resolve_position(&self, position: &str) -> Result<u32, String>;

    /// Resolve multiple position references to indices.
    fn resolve_positions(&self, positions: &[String]) -> Result<Vec<u32>, String> {
        positions
            .iter()
            .map(|pos| self.resolve_position(pos))
            .collect()
    }
}

impl PositionResolver for KeyPositionMap {
    fn resolve_position(&self, position: &str) -> Result<u32, String> {
        KeyPositionMap::resolve_position(self, position)
    }
}

/// A no-op resolver that only accepts numeric positions.
#[derive(Debug, Clone, Copy, Default)]
pub struct NumericOnlyResolver;

impl PositionResolver for NumericOnlyResolver {
    fn resolve_position(&self, position: &str) -> Result<u32, String> {
        position
            .parse::<u32>()
            .map_err(|_| format!("expected numeric position, got: {}", position))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HEADER: &str = r#"
#define POS_LH_T1 52
#define POS_LH_T2 53
#define POS_LH_C6R1 0
#define POS_LH_C5R1 1
#define POS_RH_T1 57
#define POS_RH_C6R1 9
"#;

    #[test]
    fn parses_header() {
        let map = KeyPositionMap::from_header(SAMPLE_HEADER);
        assert_eq!(map.len(), 6);
        assert_eq!(map.get("LH_T1"), Some(52));
        assert_eq!(map.get("LH_C6R1"), Some(0));
        assert_eq!(map.get("RH_T1"), Some(57));
    }

    #[test]
    fn handles_pos_prefix() {
        let map = KeyPositionMap::from_header(SAMPLE_HEADER);
        assert_eq!(map.get("POS_LH_T1"), Some(52));
        assert_eq!(map.get("LH_T1"), Some(52));
    }

    #[test]
    fn reverse_lookup() {
        let map = KeyPositionMap::from_header(SAMPLE_HEADER);
        assert_eq!(map.get_name(52), Some("LH_T1"));
        assert_eq!(map.get_name(0), Some("LH_C6R1"));
        assert_eq!(map.get_name(999), None);
    }

    #[test]
    fn resolves_mixed_positions() {
        let map = KeyPositionMap::from_header(SAMPLE_HEADER);
        let positions = vec![
            "LH_T1".to_string(),
            "52".to_string(),
            "POS_RH_T1".to_string(),
        ];
        let resolved = map.resolve_positions(&positions).unwrap();
        assert_eq!(resolved, vec![52, 52, 57]);
    }

    #[test]
    fn errors_on_unknown_position() {
        let map = KeyPositionMap::from_header(SAMPLE_HEADER);
        let result = map.resolve_position("UNKNOWN_KEY");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown position"));
    }

    #[test]
    fn numeric_only_resolver() {
        let resolver = NumericOnlyResolver;
        assert_eq!(resolver.resolve_position("42"), Ok(42));
        assert!(resolver.resolve_position("LH_T1").is_err());
    }

    #[test]
    fn loads_from_glove80_profile() {
        let profile = KeyboardProfileDoc::load("glove80").expect("glove80 profile");
        let map = KeyPositionMap::from_profile(&profile);

        // Verify Glove80 has 80 keys mapped
        assert_eq!(map.len(), 80);

        // Check some known positions
        assert_eq!(map.get("LH_C6R1"), Some(0));
        assert_eq!(map.get("LH_T1"), Some(52));
        assert_eq!(map.get("RH_T1"), Some(57));
        assert_eq!(map.get("RH_C6R6"), Some(79));
    }
}
