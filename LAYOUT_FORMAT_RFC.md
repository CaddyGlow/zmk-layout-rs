# Layout Bundle RFC

Goal: keep users on JSON while supporting MoErgo and other layout formats, avoid direct DTSI editing, and make firmware-dependent includes/defines explicit.

## Pain Today

- MoErgo JSON is tolerated but noisy (metadata, flags, inline DTS snippets) and mixes user intent with build mechanics.
- DTSI remains the bridge format even though users do not want to edit it, and it hides which firmware/toolchain knobs were applied.
- Includes/defines that depend on firmware choice are implicit; adding another layout format would repeat ad‑hoc adapters.

## Proposed Representation: Layout Bundle

Make a single canonical document that wraps the normalized layout plus format/build context. It stays JSON so MoErgo users can round‑trip, but it is structured enough for other adapters.

```
{
  "format_version": "layout-bundle/2025-02-01",
  "metadata": {
    "title": "TailorKey",
    "keyboard": "glove80",
    "locale": "en-US",
    "tags": ["qwerty", "tailorkey"],
    "source_format": "moergo-json",
    "maintainer": "moosy"
  },
  "layout": { /* existing standard adapter shape: layers, combos, behaviors, macros, inputs */ },
  "overlays": {
    "custom_devicetree": "/* raw DTS from MoErgo customDevicetree */",
    "custom_behaviors": "/* raw DTS from customDefinedBehaviors */",
    "custom_macros": "/* raw DTS from user macros */",
    "input_listeners": "/* rendered dtsi or structured list if we add one */",
    "fragments": { "key_position_header": "/* contents of key_position.h */" }
  },
  "symbols": {
    "defines": { "BT_DISC_CMD": true },
    "includes": [
      "<dt-bindings/zmk/outputs.h>",
      "<dt-bindings/zmk/keys.h>",
      "behaviors.dtsi"
    ],
    "search_paths": ["templates/glove80/"],
    "template_vars": { "keyboard_name": "Glove80" }
  },
  "targets": [
    {
      "id": "moergo-stable",
      "kind": "moergo",
      "firmware": { "repo": "moergo-sc/zmk", "channel": "stable", "version": "v25.05", "board": "glove80" },
      "template": "templates/glove80/keymap.dtsi.j2",
      "overlays": ["custom_devicetree", "custom_behaviors", "custom_macros", "input_listeners", "fragments"],
      "defines": ["BT_DISC_CMD"],
      "includes": ["<behaviors.dtsi>"],
      "output": { "format": "dtsi" }
    },
    {
      "id": "upstream-zmk",
      "kind": "zmk",
      "firmware": { "repo": "zmkfirmware/zmk", "branch": "main", "board": "nice_nano_v2", "shield": "good_split" },
      "template": "templates/generic/keymap.j2",
      "overlays": ["custom_devicetree", "custom_behaviors"],
      "output": { "format": "dtsi" }
    }
  ],
  "sources": {
    "moergo_json": {
      "path": "layout.moergo.json",
      "schema_version": "1",
      "fingerprint": "sha256:...",
      "notes": "kept for round-trip back to the MoErgo editor"
    }
  }
}
```

### Notes

- `layout` stays the standard adapter format; all new pieces are additive metadata.
- `targets` make firmware/toolchain intent explicit so defines/includes and template selection are deterministic.
- `overlays` capture the raw DTS-ish sections that MoErgo JSON allows without forcing users to edit them inline.
- `sources` preserve origin for round-tripping (MoErgo) and future imports (e.g., Via/Vial/KeymapDB).

## How Adapters Use It

- **MoErgo export**: render `layout` + selected `target` back into MoErgo JSON, rehydrating DTS snippets and firmware flags from `overlays`/`symbols`.
- **DTSI render**: pick a `target`, apply its includes/defines/template, and render DTSI without losing bundle metadata; DTSI stays an artifact, not the source of truth.
- **Future formats**: add a new adapter that reads/writes `layout` while mapping its own per-format fragments into `overlays`/`symbols` and recording provenance in `sources`.

## Implementation Sketch

1. **Schema stub**: define `LayoutBundle` struct (format_version, metadata, layout, overlays, symbols, targets, sources) plus serde validation; keep `layout` as the existing `AdapterLayout`.
2. **MoErgo adapter**: write bundle import/export that cleanly separates layout content from MoErgo-specific DTS snippets and firmware hints.
3. **Rendering path**: update DTS import/export to operate on bundles (`bundle.render(target_id, template_override?)`) and make CLI default to bundle I/O; keep legacy paths as thin adapters.
4. **Docs & fixtures**: document the bundle schema, add MoErgo round-trip fixtures, and show how firmware selection toggles includes/defines.
