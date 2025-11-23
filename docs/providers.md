# Provider Module Guide

The provider stack is split into focused modules to keep responsibilities clear:

- `providers::layers` — `KeymapProvider`/`KeymapDocument` plus layer editing (bindings, metadata, movement, defines).
- `providers::combos` — combo enumeration helpers and condition/comment handling.
- `providers::behaviors` — behavior enumeration with labels, timings, binding cells, and property capture.
- `providers::format` — shared binding normalization/formatting and numeric list helpers.
- `providers::util` — reusable AST utilities for locating/creating nodes/properties.

Use `KeymapProvider`/`KeymapDocument` for mutations, and the read-only combo/behavior providers for introspection. The `BindingFormat` helper keeps binding normalization consistent across modules.
