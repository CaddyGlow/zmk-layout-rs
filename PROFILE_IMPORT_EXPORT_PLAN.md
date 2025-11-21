# Plan: Update Layout Import/Export to Use Profile Data

## Overview

This plan outlines the complete integration of `KeyboardProfileDoc` data into the layout import/export system. The goal is to enable profile-aware validation, metadata enrichment, and formatting during JSON import/export operations.

## Background

### Current State
- Import/export converts between DTS (device tree) and JSON format
- Templates (Jinja2) are used to render keymaps
- Profiles exist but are only used for template path resolution
- No validation against profile hardware specs
- No profile metadata embedded in exported JSON

### Desired State
- Full profile integration with validation
- Profile metadata embedded in exported JSON
- Profile-aware formatting and validation
- Behavior/combo/macro metadata from profiles included
- Lua API supports profile-aware operations

## User Requirements
- **Scope:** Full profile-aware system with validation, metadata embedding, and formatting
- **Compatibility:** Update existing functions to accept optional profile parameter
- **Lua API:** Update to expose profile-aware import/export

---

## Implementation Phases

### Phase 1: Extend Data Structures

**File:** `src/adapters/standard/types.rs`

**Changes to `LayoutMetadata` struct:**

Add new optional fields to capture profile information:

```rust
pub struct LayoutMetadata {
    // Existing fields
    pub title: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub extras: serde_json::Map<String, serde_json::Value>,

    // New profile-related fields
    pub keyboard: Option<String>,              // From profile.keyboard
    pub key_count: Option<u32>,                // From profile.hardware.key_count
    pub is_split: Option<bool>,                // From profile.hardware.is_split
    pub default_firmware: Option<String>,      // From profile.firmware.default
    pub boards: Vec<String>,                   // From profile.hardware.boards[].id
}
```

**New structs for behavior/combo/macro metadata:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorMetadata {
    pub name: String,
    pub description: Option<String>,
    pub params: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComboMetadata {
    pub name: String,
    pub description: Option<String>,
    pub key_positions: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroMetadata {
    pub name: String,
    pub description: Option<String>,
}
```

Add these to `AdapterLayout`:

```rust
pub struct AdapterLayout {
    pub layers: Vec<LayerSpec>,
    pub combos: Vec<ComboSpec>,
    pub behaviors: Vec<BehaviorSpec>,
    pub macros: Vec<MacroSpec>,
    pub input_listeners: Vec<InputListenerSpec>,
    pub metadata: LayoutMetadata,

    // New: metadata from profile
    pub profile_behaviors: Vec<BehaviorMetadata>,
    pub profile_combos: Vec<ComboMetadata>,
    pub profile_macros: Vec<MacroMetadata>,
}
```

**Tests to add:**
- Serialization/deserialization of new metadata fields
- Ensure backwards compatibility when fields are None

---

### Phase 2: Update Core Import Functions

**File:** `src/adapters/standard/io.rs`

**Function signature changes:**

```rust
// Before:
pub fn import_standard_str(json: &str, base: &DtsDocument) -> Result<DtsDocument, AdapterError>

// After:
pub fn import_standard_str(
    json: &str,
    base: &DtsDocument,
    profile: Option<&KeyboardProfileDoc>,
    profile_root: Option<&Path>,
) -> Result<DtsDocument, AdapterError>
```

Similarly update:
- `import_standard_file()`
- `import_standard_str_with_template()`
- `import_standard_file_with_template()`
- `import_standard_file_for_profile()` (use new validation logic)

**New validation logic when profile is Some():**

1. **Key count validation:**
   - Count total bindings across all layers
   - Compare against `profile.hardware.key_count`
   - Return error if mismatch

2. **Key position validation:**
   - Extract key positions from `profile.layout.formatting`
   - Validate that all binding positions are valid
   - Return error for out-of-range positions

3. **Template resolution:**
   - If profile provided, use `profile.layout.template`
   - Override any template path in JSON

4. **Metadata extraction:**
   - Extract behavior/combo/macro definitions from profile
   - Add to AdapterLayout for reference

**Error types to add:**

```rust
pub enum AdapterError {
    // Existing variants...

    // New validation errors
    KeyCountMismatch { expected: u32, actual: u32 },
    InvalidKeyPosition { position: u32, max: u32 },
    ProfileValidationFailed(String),
}
```

**Implementation approach:**
- Create helper function `validate_with_profile(&AdapterLayout, &KeyboardProfileDoc) -> Result<(), AdapterError>`
- Call from import functions when profile is Some()
- Keep existing behavior when profile is None

---

### Phase 3: Update Core Export Functions

**File:** `src/adapters/standard/io.rs`

**Function signature changes:**

```rust
// Before:
pub fn export_standard_str(document: &DtsDocument) -> Result<String, AdapterError>

// After:
pub fn export_standard_str(
    document: &DtsDocument,
    profile: Option<&KeyboardProfileDoc>,
) -> Result<String, AdapterError>
```

Similarly update:
- `export_standard_file()`
- `export_standard_str_with_template()`
- `export_standard_file_with_template()`

**New enrichment logic when profile is Some():**

1. **Embed profile metadata:**
   - Set `metadata.keyboard = Some(profile.keyboard)`
   - Set `metadata.key_count = Some(profile.hardware.key_count)`
   - Set `metadata.is_split = Some(profile.hardware.is_split)`
   - Set `metadata.default_firmware = Some(profile.firmware.default)`
   - Set `metadata.boards = profile.hardware.boards.iter().map(|b| b.id).collect()`

2. **Include profile definitions:**
   - Add `profile_behaviors` from `profile.behaviors`
   - Add `profile_combos` from `profile.combos`
   - Add `profile_macros` from `profile.macros`

3. **Apply formatting rules:**
   - Use `profile.layout.formatting` for key position layout
   - Format output for readability according to profile preferences

**JSON output example with profile metadata:**

```json
{
  "title": "My Glove80 Layout",
  "description": "Custom layout for coding",
  "keyboard": "glove80",
  "keyCount": 80,
  "isSplit": true,
  "defaultFirmware": "v25.05",
  "boards": ["glove80_lh", "glove80_rh"],
  "layers": [...],
  "combos": [...],
  "behaviors": [...],
  "macros": [...],
  "profileBehaviors": [
    {"name": "td", "description": "Tap dance", "params": ["binding1", "binding2"]}
  ],
  "profileCombos": [...],
  "profileMacros": [...]
}
```

---

### Phase 4: Enhance AdapterLayout Conversion

**File:** `src/adapters/standard/layout.rs`

**Update `from_standard_json()`:**

```rust
// Before:
pub fn from_standard_json(json: &StandardFormat) -> Result<Self, AdapterError>

// After:
pub fn from_standard_json(
    json: &StandardFormat,
    profile: Option<&KeyboardProfileDoc>,
) -> Result<Self, AdapterError>
```

**Changes:**
- Extract profile metadata fields from JSON if present
- Call validation helper if profile provided
- Populate `profile_behaviors`, `profile_combos`, `profile_macros` from JSON

**Update `to_standard_json()`:**

```rust
// Before:
pub fn to_standard_json(&self) -> Result<StandardFormat, AdapterError>

// After:
pub fn to_standard_json(
    &self,
    profile: Option<&KeyboardProfileDoc>,
) -> Result<StandardFormat, AdapterError>
```

**Changes:**
- Enrich metadata from profile if provided
- Include profile behavior/combo/macro metadata
- Apply formatting preferences

**Update `StandardFormat` struct:**

Add fields to match extended `AdapterLayout`:

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardFormat {
    pub title: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,

    // New profile metadata
    pub keyboard: Option<String>,
    pub key_count: Option<u32>,
    pub is_split: Option<bool>,
    pub default_firmware: Option<String>,
    pub boards: Option<Vec<String>>,

    pub layers: Vec<LayerSpec>,
    pub combos: Vec<ComboSpec>,
    pub behaviors: Vec<BehaviorSpec>,
    pub macros: Vec<MacroSpec>,
    pub input_listeners: Vec<InputListenerSpec>,

    // Profile reference data
    pub profile_behaviors: Option<Vec<BehaviorMetadata>>,
    pub profile_combos: Option<Vec<ComboMetadata>>,
    pub profile_macros: Option<Vec<MacroMetadata>>,

    #[serde(flatten)]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}
```

---

### Phase 5: Update Lua API

**File:** `src/lua_api/api.rs`

**Current `load_json()` signature (around line 118):**

```rust
pub fn load_json(&mut self, path: String) -> LuaResult<()>
```

**New signature:**

```rust
pub fn load_json(&mut self, path: String, profile_name: Option<String>) -> LuaResult<()>
```

**Implementation changes:**

```rust
pub fn load_json(&mut self, path: String, profile_name: Option<String>) -> LuaResult<()> {
    // Load profile if name provided
    let profile = if let Some(name) = profile_name {
        // Use existing profile loading logic from manifest
        Some(load_keyboard_profile(&name)
            .map_err(|e| LuaError::RuntimeError(format!("Failed to load profile: {}", e)))?)
    } else {
        None
    };

    // Determine profile_root path
    let profile_root = profile.as_ref().map(|p| {
        // Get profile root from manifest or use default
        Path::new("firmware_profiles")
    });

    // Call import with profile
    import_standard_file(
        &path,
        &self.base_document,
        profile.as_ref(),
        profile_root,
    )
    .map_err(|e| LuaError::RuntimeError(format!("Import failed: {}", e)))?;

    // ... rest of existing logic
    Ok(())
}
```

**Current `save_json()` signature (around line 138):**

```rust
pub fn save_json(&self, path: String) -> LuaResult<()>
```

**New signature:**

```rust
pub fn save_json(&self, path: String, profile_name: Option<String>) -> LuaResult<()>
```

**Implementation changes:**

```rust
pub fn save_json(&self, path: String, profile_name: Option<String>) -> LuaResult<()> {
    // Load profile if name provided
    let profile = if let Some(name) = profile_name {
        Some(load_keyboard_profile(&name)
            .map_err(|e| LuaError::RuntimeError(format!("Failed to load profile: {}", e)))?)
    } else {
        None
    };

    // Call export with profile
    export_standard_file(
        &self.current_document,
        &path,
        profile.as_ref(),
    )
    .map_err(|e| LuaError::RuntimeError(format!("Export failed: {}", e)))?;

    Ok(())
}
```

**Lua usage examples:**

```lua
-- Load JSON without profile (existing behavior)
editor:load_json("layout.json")

-- Load JSON with profile validation
editor:load_json("layout.json", "glove80")

-- Save JSON without profile
editor:save_json("output.json")

-- Save JSON with profile metadata
editor:save_json("output.json", "glove80")
```

---

### Phase 6: Update Tests

**File:** `tests/adapters.rs`

**Tests to update:**

1. **Existing tests (ensure backward compatibility):**
   - Update all `import_standard_*()` calls to pass `None` for profile
   - Update all `export_standard_*()` calls to pass `None` for profile
   - Ensure tests still pass with no profile provided

2. **New profile-aware import tests:**
   - `test_import_with_profile_validation()`
     - Import valid JSON with matching profile
     - Verify metadata is populated
   - `test_import_key_count_mismatch()`
     - Import JSON with wrong key count
     - Verify error is returned
   - `test_import_invalid_key_position()`
     - Import JSON with out-of-range key position
     - Verify error is returned

3. **New profile-aware export tests:**
   - `test_export_with_profile_metadata()`
     - Export DTS with profile
     - Verify JSON contains profile metadata fields
   - `test_export_includes_profile_behaviors()`
     - Export with profile
     - Verify profile behaviors/combos/macros in JSON

4. **Round-trip tests:**
   - `test_roundtrip_with_profile()`
     - Import JSON with profile
     - Export with same profile
     - Verify metadata preserved

**Test fixtures needed:**
- Create test profile TOML in `tests/fixtures/`
- Create test JSON layouts (valid and invalid)
- Mock KeyboardProfileDoc for unit tests

---

### Phase 7: Update Documentation

**File:** `README.md`

Add section on profile-aware import/export:

```markdown
## Profile-Aware Import/Export

The import/export system supports optional profile integration for validation and metadata enrichment.

### Import with Profile

When importing JSON with a profile, the system validates:
- Key count matches profile hardware specs
- Key positions are within valid range
- Bindings reference valid behaviors

```rust
use zmk_layout::adapters::standard::import_standard_file;
use zmk_layout::profiles::load_keyboard_profile;

let profile = load_keyboard_profile("glove80")?;
let document = import_standard_file(
    "layout.json",
    &base_document,
    Some(&profile),
    Some(Path::new("firmware_profiles")),
)?;
```

### Export with Profile

When exporting with a profile, the JSON includes:
- Keyboard name and hardware specs
- Default firmware version
- Board identifiers
- Behavior/combo/macro metadata from profile

```rust
let profile = load_keyboard_profile("glove80")?;
export_standard_file(&document, "output.json", Some(&profile))?;
```

### Lua API

```lua
-- Import with validation
editor:load_json("layout.json", "glove80")

-- Export with metadata
editor:save_json("output.json", "glove80")
```
```

**File:** `CHANGELOG.md`

Add entry:

```markdown
## [Unreleased]

### Changed
- **BREAKING:** Import/export functions now accept optional `KeyboardProfileDoc` parameter
  - `import_standard_str()`, `import_standard_file()`, etc. now take `profile: Option<&KeyboardProfileDoc>`
  - `export_standard_str()`, `export_standard_file()`, etc. now take `profile: Option<&KeyboardProfileDoc>`
  - Pass `None` to maintain previous behavior

### Added
- Profile-aware validation during import (key count, key positions)
- Profile metadata embedding in exported JSON (keyboard name, firmware, hardware specs)
- Behavior/combo/macro metadata from profiles included in exports
- Lua API methods `load_json()` and `save_json()` now accept optional profile name
- New error types for validation failures: `KeyCountMismatch`, `InvalidKeyPosition`

### Enhanced
- `LayoutMetadata` now includes profile fields: `keyboard`, `key_count`, `is_split`, `default_firmware`, `boards`
- `AdapterLayout` includes profile reference data: `profile_behaviors`, `profile_combos`, `profile_macros`
- `StandardFormat` JSON schema extended with profile fields
```

---

## Implementation Order

1. **Phase 1** - Data structures (types.rs)
   - Low risk, foundation for other changes
   - Can be tested in isolation

2. **Phase 4** - AdapterLayout conversion (layout.rs)
   - Depends on Phase 1
   - Core conversion logic

3. **Phase 2** - Import functions (io.rs imports)
   - Depends on Phase 1 and 4
   - Add validation logic

4. **Phase 3** - Export functions (io.rs exports)
   - Depends on Phase 1 and 4
   - Add enrichment logic

5. **Phase 6** - Tests (tests/adapters.rs)
   - Test each phase incrementally
   - Update existing tests first, then add new ones

6. **Phase 5** - Lua API (lua_api/api.rs)
   - Depends on Phases 2 and 3
   - Higher-level integration

7. **Phase 7** - Documentation (README.md, CHANGELOG.md)
   - Final step once all code is working

---

## Key Design Decisions

### Optional Profile Parameter
All functions accept `Option<&KeyboardProfileDoc>`:
- `None` = backward compatible behavior, no validation
- `Some(profile)` = full validation and enrichment

### Profile Root Path
- Import functions need `profile_root` to resolve template paths
- Use `Option<&Path>` to handle cases where profile is provided but templates aren't needed

### Validation Strategy
- Fail fast: validation errors stop import immediately
- Clear error messages: include expected vs actual values
- Non-breaking: validation only occurs when profile is Some()

### Metadata Embedding
- All profile metadata is optional in JSON
- Backward compatible: old JSON files can be imported
- Forward compatible: new fields ignored by old parsers

### Lua API Design
- Profile name as string, not profile object (easier for Lua users)
- Automatic profile loading within Rust code
- Optional parameter maintains backward compatibility

---

## Risk Assessment

### Low Risk
- Phase 1 (data structures) - additive changes only
- Phase 7 (documentation) - no code changes

### Medium Risk
- Phase 2, 3 (import/export) - signature changes affect callers
- Phase 4 (conversion) - core logic changes, good test coverage needed

### Higher Risk
- Phase 5 (Lua API) - FFI boundary, harder to debug
- Phase 6 (tests) - must cover all edge cases

### Mitigation
- Implement incrementally, test after each phase
- Keep backward compatibility via Option<> parameters
- Add comprehensive error handling and messages
- Use existing test fixtures where possible

---

## Success Criteria

1. All existing tests pass with minimal changes (only adding `None` parameters)
2. New tests validate profile integration works correctly
3. Validation catches mismatches between JSON and profile specs
4. Exported JSON includes all relevant profile metadata
5. Lua API successfully uses profiles for import/export
6. Documentation clearly explains new functionality
7. Error messages are clear and actionable

---

## Future Enhancements (Out of Scope)

- Profile-specific formatter preferences for JSON output
- Automatic profile detection from JSON metadata
- Profile versioning and migration support
- CLI commands for profile-aware import/export
- Visual diff of profile vs layout differences
