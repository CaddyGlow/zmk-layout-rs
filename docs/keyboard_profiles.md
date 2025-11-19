# Keyboard Profile Specification

This document explains what we mean by a **keyboard profile** inside
`zmk-layout-rs`, what problems it solves, and how to keep profiles simple while
still capturing everything we need to flash dependable firmware.

## Goals
- Single source of truth for a keyboard’s hardware facts, preferred firmware,
  behavior catalog, macros/combos, and layout templates.
- Keep manifests (`firmware_profiles/*.toml`) declarative by pointing at the
  correct profile instead of re‑describing the keyboard in every workflow.
- Provide enough structure for tooling (CLI, GUI, docs) without building a
  complex schema that is hard to maintain.

The `example_profile/` directory already demonstrates these ideas for the Glove80
and Corne. The spec below formalizes the minimum expectations so new keyboards
can follow the same pattern.

## Directory Structure

A keyboard profile is a folder that carries a root YAML file (e.g.
`example_profile/glove80.yaml` or `example_profile/glove80/main.yaml`) plus
referenced components:

```
<keyboard>/
├── main.yaml          # Entry point referenced by tooling/manifests
├── hardware.yaml      # Physical keyboard description
├── firmwares.yaml     # Supported firmware branches/tags
├── strategies.yaml    # Allowed build methods/toolchains
├── behaviors.yaml     # Behavior/macro catalog
├── keymap.yaml        # Formatting + template metadata
└── config/…           # Shared templates, macros, combos, includes
```

The YAML loader only needs to support `includes` so profiles can stay modular.
Simple keyboards can keep everything in a single `<keyboard>.yaml`; larger
keyboards can drop files next to `main.yaml` the way `example_profile/glove80/`
does.

## Required Top-Level Fields

Every root YAML file must provide these keys:

| Field          | Purpose                                                        |
|----------------|----------------------------------------------------------------|
| `keyboard`     | Stable identifier (used by manifests and the CLI).             |
| `description`  | Human-readable summary.                                        |
| `vendor`       | Manufacturer/designer attribution.                             |
| `key_count`    | Integer number of physical keys.                               |
| `is_split`     | Boolean – informs tooling about halves vs. monolithic boards.  |
| `includes`     | Ordered list of component YAML files to merge.                 |

Optional but recommended:

- `compile_methods`: List of build strategies (`method_type`, repo/branch, and
  board/shield defaults). `example_profile/corne.yaml` shows the structure.
- `flash_methods`: Enumerates USB/DFU/etc. flashing recipes.
- `firmwares`: Inline firmware catalog if you do not split into
  `firmwares.yaml`.
- `keymap`: Formatting hints plus template file (`keymap_dtsi_file`) used
  by `zmk-layout` generators.

## Firmware Catalog

Profiles own the list of firmware versions that make sense for that keyboard.
Each entry should describe at minimum:

```yaml
firmwares:
  v25.05:
    version: "v25.05"
    description: "Stable MoErgo firmware v25.05"
    build_options:
      repository: "moergo-sc/zmk"
      branch: "v25.05"
    # Optional overrides
    kconfig: { ... }        # per-firmware config knobs
    combos: [ ... ]         # extra combos enabled only on this firmware
    macros: [ ... ]         # firmware-defined macros
```

Firmware metadata feeds both the layout tooling (to render version pickers or
warnings) and the manifest (`firmware_profiles/*.toml`), which only needs the
default firmware id plus a pointer back to the profile (see
`firmware_profiles/glove80.toml` → `profile = "keyboards/glove80/main.yaml"`).

## Hardware + Layout Metadata

`hardware.yaml` captures everything that does not change per build:

- Physical key order/rows (`keymap.formatting.rows`).
- Flash/DFU instructions (USB queries, timeouts, mass storage paths).
- Build configuration (board/shield combinations, `CONFIG_ZMK_SPLIT_ROLE`
  definitions, artifact naming).

`keymap.yaml` records serializer hints and includes the Devicetree template for
layout generation. These files let the CLI render consistent keymaps without
relying on ad-hoc scripts.

## Combos, Behaviors, Macros

Keyboard-specific combos or macros live under `config/` (shared for all
keyboards) or in the profile folder itself. To keep the spec lightweight:

- Use `behaviors.yaml` to enumerate reusable behaviors/macros. Each entry should
  include a name, `code`, description, and expected parameters (see
  `example_profile/glove80/behaviors.yaml` for a concrete list).
- Combos that are part of the “stock” layout can ship as YAML fragments inside
  `config/common/*.yaml`, then be referenced from `includes`.
- Firmware‑specific combos/macros may sit under each firmware entry (e.g.
  `firmwares.pr36.combos = [...]`) so tooling can toggle them when that firmware
  is selected.

Because everything is plain YAML/TOML, we can parse the same structures inside
Rhai scripts, CLI commands, or future GUIs without introducing yet another
format.

## Keeping It Simple

- Favor descriptive text over deeply nested schemas. Most sections only need a
  `method_type`, `repository`, `branch`, and optional maps for environment or
  Kconfig overrides.
- Use shared includes (`example_profile/config/all.yaml`) for items that span
  multiple keyboards (standard behaviors, default macros, template paths).
- If a keyboard does not require a concept (e.g., no special flash modes), omit
  the section entirely; defaults kick in downstream.
- When in doubt, match the structure already checked into
  `example_profile/glove80/`—new keyboards can copy that folder, delete unused
  pieces, and fill in their specifics.

This spec gives us enough structure to teach tooling how to list keyboards,
display supported firmware, parameterize builds, and pre-fill combo/macro
libraries without forcing profile authors to learn a new DSL.
