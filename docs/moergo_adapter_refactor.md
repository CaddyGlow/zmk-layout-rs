# MoErgo Adapter Refactor Plan

## Goals
- Treat MoErgo JSON as a vendor-specific format with clear boundaries.
- Normalize config parameter aliases into canonical KConfig defines when importing.
- Re-alias canonical defines back to MoErgo keys when exporting.
- Keep bundle logic firmware-agnostic; MoErgo-specific handling lives in one place.
- Preserve template defaults in the MoErgo module while allowing overrides.

## Design Approach
- **Module split**: Introduce `src/adapters/moergo/` with:
  - `spec.rs`: serde structs mirroring the MoErgo JSON payload (document source/links).
  - `mapping.rs`: alias ↔ canonical KConfig map generated at build time from a TOML copy of `keyboards/config/common/all_kconfig.yaml` (embedded), with an optional on-disk override when the file exists; no runtime YAML parsing. The TOML copy lives under `profiles/vendors/moergo/`.
  - `adapter.rs`: import/export helpers that convert between MoErgo JSON and `LayoutBundle`, handling overlays, symbols, targets, and templates.
  - `behaviors.rs` (or data loader): optional helpers that read `profiles/vendors/moergo/behaviors.json` (from the MoErgo editor) to map behaviors to required header includes/metadata.
  - `template.rs` (or constants): default template path (`templates/glove80/keymap.dtsi.j2`) plus helper to apply overrides.
- **Bundle layer**: `bundle.rs` delegates to the MoErgo adapter for JSON import/export. Bundle remains unaware of alias logic.
- **Mapping strategy**:
  - Import: `config_parameters` → alias map → canonical names inserted into `symbols.defines`; default target `defines` lists the canonical keys. Unknown params fall back to normalization rules below.
  - Export: bundle `symbols.defines` → look up canonical → MoErgo alias; emit aliases into `config_parameters`. If a define lacks a known alias, warn and emit the normalized canonical key instead of skipping.
  - Fallback normalization: unmapped params keep their value; if the key starts with `CONFIG_ZMK_`, keep as-is, otherwise prefix `CONFIG_ZMK_` before storing/exporting so nothing is dropped.
  - Preserve original values; default to `true` when missing as today. Preserve input order when available; deterministic order is acceptable otherwise.
- **Template handling**:
  - Default template path lives in the MoErgo module.
  - Targets may override; the module documents how (CLI flag or target template override).
- **DTSI-derived values**:
  - Any logic that imports a DTSI and tries to “discover” variables/includes/defines should live in the MoErgo module (vendor-specific). We will rewrite that flow alongside the adapter split so the generic bundle stays unaware of DTSI heuristics.

## Implementation Notes
- **Filesystem layout**: Vendor assets (embedded TOML + optional override) live under `profiles/vendors/moergo/`. Profiles now live under `profiles/keyboards` and `profiles/firmwares` (previously `keyboard_profiles`/`firmware_profiles`); update consumers accordingly.
- **Mapping source**: Convert `keyboards/config/common/all_kconfig.yaml` to TOML (stored at `profiles/vendors/moergo/all_kconfig.toml`), and generate a Rust map at build time (alias ↔ canonical). Load inverses for export. At runtime, first check for the TOML file on disk to override the embedded default; no runtime YAML parsing.
- **Behavior metadata**: Ship `profiles/vendors/moergo/behaviors.json` (from the MoErgo editor) so the adapter can add required header includes (e.g., dt-bindings) and surface behavior metadata when importing/exporting layouts.
- **Symbols contract**:
  - `symbols.defines`: canonical KConfig names for MoErgo imports (e.g., `HID_POINTING` alias → `CONFIG_ZMK_POINTING` stored).
  - `symbols.includes/search_paths/template_vars`: unchanged; `template_vars.locale` still set from payload.
  - Targets’ `defines` lists reference canonical names only.
- **Error handling/logging**:
  - Unknown aliases on import: warn (verbosity-controlled) and normalize the key using the `CONFIG_ZMK_` prefix rule, preserving the value (no failure).
  - Unknown canonical on export: warn and emit the normalized key (prefixing `CONFIG_ZMK_` when absent) when no alias exists, rather than skipping.
- **Testing**:
  - Fixtures with aliased `config_parameters` (array and object forms) should import to canonical `symbols.defines` and re-export with the original aliases.
  - Existing bundle tests for overlays/defines should continue to pass.

## Impact
- Cleaner separation: vendor logic lives in `adapters/moergo`, reducing bundle coupling.
- Users/Bundles see canonical define names; MoErgo editor still receives its aliases on export.
- Template defaults are centralized and documented.
- Default map is embedded; optional runtime TOML load only occurs when an override file is present (no runtime YAML).

## New/Adjusted Symbols
- `symbols.defines`: canonical KConfig names derived from MoErgo aliases. Example: `HID_POINTING` alias → `CONFIG_ZMK_POINTING` (canonical stored). Unmapped params are stored as `CONFIG_ZMK_*` (prefix added when missing) to keep them round-trippable.
- Target `defines`: mirrors the canonical names from `symbols.defines`.
- `symbols.template_vars`: still includes `locale` from MoErgo payload. No new keys needed for mapping.

## Open Questions
- Sort/export order for `config_parameters`: try to preserve source order when available; deterministic order is acceptable if not.
- Should we allow an override path/env beyond `profiles/vendors/moergo/all_kconfig.toml`, and should we cache the override across invocations?
- Verbosity: warnings for unknown aliases/canonicals should respect `--verbose`/quiet modes.
