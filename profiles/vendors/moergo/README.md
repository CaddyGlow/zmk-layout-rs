# MoErgo vendor assets

This directory holds vendor-provided assets for MoErgo. The MoErgo adapter embeds `all_kconfig.toml` from here at build time and will prefer an on-disk override if present.

- `behaviors.json`: Behavior metadata (names, descriptions, includes, params, required configs) sourced from the MoErgo editor.
- Glove80 key coordinates now live alongside the profile at `profiles/keyboards/glove80/key_positions.json`.
