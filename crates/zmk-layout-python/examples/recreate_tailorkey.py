#!/usr/bin/env python3
"""Recreate the TailorKey sample keymap using code and small data modules."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any, Dict, List

import zmk_layout

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(Path(__file__).parent))

from tailorkey_layers import base, layer_names, overrides  # noqa: E402
from tailorkey_behaviors import hold_taps, macros  # noqa: E402
from tailorkey_combos import combos, input_listeners, metadata  # noqa: E402


def build_layers() -> List[Dict[str, Any]]:
    layers = [{"name": layer_names[0], "bindings": base}]
    for name in layer_names[1:]:
        bindings = ["&trans"] * len(base)
        for idx, val in overrides.get(name, {}).items():
            bindings[idx - 1] = val
        layers.append({"name": name, "bindings": bindings})
    return layers


def build_macros() -> List[Dict[str, Any]]:
    result = []
    for macro in macros:
        cells = len(macro.get("params", []))
        result.append(
            {
                "name": macro["name"],
                "description": macro.get("description", ""),
                "bindings": macro.get("bindings", []),
                "wait_ms": macro.get("wait_ms"),
                "tap_ms": macro.get("tap_ms"),
                "binding_cells": cells or None,
                "compatible": (
                    "zmk,behavior-macro-one-param" if cells else "zmk,behavior-macro"
                ),
            }
        )
    return result


def fmt_prop(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return f"< {value} >"
    if isinstance(value, list):
        return "< " + " ".join(str(v) for v in value) + " >"
    if isinstance(value, str):
        return f'"{value}"'
    return str(value)


def build_behaviors() -> List[Dict[str, Any]]:
    result = []
    for ht in hold_taps:
        props = {
            "tapping-term-ms": fmt_prop(ht.get("tapping_term_ms")),
            "quick-tap-ms": fmt_prop(ht.get("quick_tap_ms")),
            "require-prior-idle-ms": fmt_prop(ht.get("require_prior_idle_ms")),
        }
        positions = ht.get("hold_trigger_key_positions") or []
        if positions:
            props["hold-trigger-key-positions"] = fmt_prop(positions)
        if "hold_trigger_on_release" in ht:
            props["hold-trigger-on-release"] = fmt_prop(ht.get("hold_trigger_on_release"))
        if "flavor" in ht and ht["flavor"] is not None:
            props["flavor"] = fmt_prop(ht["flavor"])
        # drop None values
        props = {k: v for k, v in props.items() if v is not None}
        result.append(
            {
                "name": ht["name"],
                "description": ht.get("description", ""),
                "compatible": "zmk,behavior-hold-tap",
                "binding_cells": 2,
                "bindings": ht.get("bindings", []),
                "properties": props,
            }
        )
    return result


def build_standard_layout() -> Dict[str, Any]:
    return {
        "layers": build_layers(),
        "combos": combos,
        "behaviors": build_behaviors(),
        "macros": build_macros(),
        "input_listeners": input_listeners,
        "metadata": metadata,
    }


def main() -> None:
    layout = zmk_layout.Layout()
    standard = build_standard_layout()
    json_payload = json.dumps(standard)

    layout.parse_json(json_payload, "examples/moergo_glove80.j2")

    output_path = ROOT / "out" / "tailorkey_from_json.keymap"
    output_path.parent.mkdir(parents=True, exist_ok=True)
    layout.save_dts(str(output_path))
    print(f"Recreated TailorKey -> {output_path}")


if __name__ == "__main__":
    main()
