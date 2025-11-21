# MoErgo vendor assets

This directory holds vendor-provided assets for MoErgo. The MoErgo adapter embeds `all_kconfig.toml` from here at build time and will prefer an on-disk override if present.

- `behaviors.json`: Behavior metadata (names, descriptions, includes, params, required configs) sourced from the MoErgo editor.
- `key_positions.json`: Key coordinate/rotation data (e.g., Glove80) as provided by the editor.
