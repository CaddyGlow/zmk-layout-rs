#!/usr/bin/env python3
"""Recreate the TailorKey sample keymap from its MoErgo JSON export by
converting it to the standard layout JSON and feeding it through the template."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict, List, Union

import zmk_layout

BindingNode = Union[str, Dict[str, Any]]


def render_binding(node: BindingNode) -> str:
    if isinstance(node, str):
        return node
    if isinstance(node, dict):
        value = node["value"]
        params = node.get("params", [])
        if not params:
            return value
        rendered = [render_binding(child) for child in params]
        return f'{value} {" ".join(rendered)}'
    raise TypeError(f"unsupported binding node: {type(node)}")


def trim_name(name: str) -> str:
    return name[1:] if name.startswith("&") else name


def format_num(value: Any) -> str:
    return f"< {value} >"


def format_num_list(values: List[Any]) -> str:
    return "< " + " ".join(str(v) for v in values) + " >"


def to_standard_layout(data: Dict[str, Any]) -> Dict[str, Any]:
    layer_names: List[str] = data.get("layer_names", [])
    layers = [
        {"name": name, "bindings": [render_binding(entry) for entry in bindings]}
        for name, bindings in zip(layer_names, data.get("layers", []))
    ]

    macros = []
    for macro in data.get("macros", []):
        cells = len(macro.get("params", []))
        macros.append(
            {
                "name": trim_name(macro["name"]),
                "description": macro.get("description", ""),
                "bindings": [render_binding(entry) for entry in macro.get("bindings", [])],
                "wait_ms": macro.get("waitMs"),
                "tap_ms": macro.get("tapMs"),
                "binding_cells": cells if cells else None,
                "compatible": "zmk,behavior-macro-one-param" if cells else "zmk,behavior-macro",
            }
        )

    behaviors = []
    for ht in data.get("holdTaps", []):
        props: Dict[str, str] = {}
        if "tappingTermMs" in ht:
            props["tapping-term-ms"] = format_num(ht["tappingTermMs"])
        if "quickTapMs" in ht:
            props["quick-tap-ms"] = format_num(ht["quickTapMs"])
        if "requirePriorIdleMs" in ht:
            props["require-prior-idle-ms"] = format_num(ht["requirePriorIdleMs"])
        if ht.get("holdTriggerKeyPositions"):
            props["hold-trigger-key-positions"] = format_num_list(
                ht["holdTriggerKeyPositions"]
            )
        if "holdTriggerOnRelease" in ht:
            props["hold-trigger-on-release"] = (
                "true" if ht["holdTriggerOnRelease"] else "false"
            )
        if "flavor" in ht:
            props["flavor"] = f"\"{ht['flavor']}\""

        behaviors.append(
            {
                "name": trim_name(ht["name"]),
                "description": ht.get("description", ""),
                "compatible": "zmk,behavior-hold-tap",
                "binding_cells": 2,
                "bindings": [render_binding(entry) for entry in ht.get("bindings", [])],
                "properties": props,
            }
        )

    combos = []
    for combo in data.get("combos", []):
        combos.append(
            {
                "name": combo["name"],
                "description": combo.get("description", ""),
                "key_positions": combo.get("keyPositions", []),
                "binding": render_binding(combo["binding"]),
                "timeout_ms": combo.get("timeoutMs"),
                "layers": combo.get("layers", []),
            }
        )

    input_listeners = []
    for listener in data.get("inputListeners", []):
        nodes = []
        for node in listener.get("nodes", []):
            nodes.append(
                {
                    "code": node["code"],
                    "description": node.get("description"),
                    "layers": node.get("layers", []),
                    "inputProcessors": [
                        {"code": proc["code"], "params": proc.get("params", [])}
                        for proc in node.get("inputProcessors", [])
                    ],
                }
            )
        input_listeners.append(
            {
                "code": listener["code"],
                "inputProcessors": listener.get("inputProcessors", []),
                "nodes": nodes,
            }
        )

    metadata = {
        "title": data.get("title"),
        "author": data.get("creator"),
        "description": data.get("notes"),
        "extras": {
            "keyboard": data.get("keyboard"),
            "uuid": data.get("uuid"),
            "parent_uuid": data.get("parent_uuid"),
            "tags": data.get("tags", []),
        },
    }

    return {
        "layers": layers,
        "combos": combos,
        "behaviors": behaviors,
        "macros": macros,
        "input_listeners": input_listeners,
        "metadata": metadata,
    }


def main() -> None:
    repo_root = Path(__file__).resolve().parents[3]
    sample_json = repo_root / "examples" / "samples" / "8e349bac-1664-41f1-8d2e-7b9398f6d8cc_TailorKey v4.2i Bilateral.json"
    template = repo_root / "examples" / "moergo_glove80.j2"
    output_path = repo_root / "out" / "tailorkey_from_json.keymap"

    data = json.loads(sample_json.read_text())
    standard = to_standard_layout(data)

    layout = zmk_layout.Layout()
    layout.parse_json(json.dumps(standard), str(template))

    output_path.parent.mkdir(parents=True, exist_ok=True)
    layout.save_dts(str(output_path))

    print(
        f"Recreated TailorKey: {len(standard['layers'])} layers, "
        f"{len(data.get('macros', []))} macros, "
        f"{len(data.get('holdTaps', []))} hold-taps, "
        f"{len(data.get('combos', []))} combos -> {output_path}"
    )


if __name__ == "__main__":
    main()
