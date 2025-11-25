# Lua Layout API

`zmk-layout keymap lua` exposes a fluent `layout` global (1-based indices) shared with `script`
tasks. Use it for staged edits, queries, and file I/O; `log("...")` remains available for notes.
Globals `ARGS`, `TASK_ID`, `TARGET`, and `COMMENT` are populated when running inside task files.

Run a script against real fixtures in this repo:

```bash
zmk-layout keymap lua \
  --script tests/fixtures/script_task_file.lua \
  --layout tests/fixtures/tasks_regression_base.dts \
  --diff
```

## Entry points
- `layout:layer(name)` – stage binding/metadata edits with `:bind`, `:bindings`, `:meta`, `:get_binding`, `:get_bindings`, `:apply`.
- `layout:combo(name)` – configure combos with `:keys`, `:binding`, `:timeout`, `:on_layers`, `:when`, `:apply`.
- `layout:behavior(name)` – update bindings/params with `:bindings`, `:param`, `:get_bindings`, `:get_param`, `:apply`.
- `layout:move_layer(name, index)` / `layout:remove_layer(name)` – reorder or drop layers (indices are 1-based).
- `layout:meta(key, value)` – write layout-level metadata extras.
- Queries: `layout:get_layer`, `layout:get_combo`, `layout:get_behavior`, `layout:list_layers`, `layout:list_combos`, `layout:list_behaviors`.
- I/O: `layout:load_dts/dtsi/json`, `layout:parse_dts/parse_json`, `layout:save_dts/save_json`, `layout:to_dts_string/to_json_string`, `layout:render_template`.
- `log("...")` records messages that surface on stderr when the CLI runs scripts.

## Builder-style `layout` API
Use the `layout` object for staged edits that mirror the Rust builders. Binding positions on
builders are 1-based for Lua ergonomics.

```lua
-- Load a DTS, update bindings + metadata, add a combo, and save the result
layout:load_dts("tests/fixtures/tasks_regression_base.dts")

layout:layer("raise")
  :bindings({"&kp TAB", "&kp Q", "&kp W"})
  :meta("display", "Raise")
  :apply()

layout:combo("copy")
  :keys({27, 28})
  :binding("&kp C")
  :on_layers({"base"})
  :when("mods.ctrl")
  :apply()

layout:behavior("caps_word")
  :bindings({"&caps_word"})
  :param("label", "Caps Word")
  :apply()

layout:save_dts("out/tasks_regression_base.generated.dts")
```

Layer builders support partial edits (`:bind(5, "&kp ESC")`), full replacement (`:bindings({...})`),
and metadata updates (`:meta("color", "blue")`). Combo builders allow timeout overrides
(`:timeout(30)`), layer masks, and multiple condition strings. Behavior builders stage bindings
plus TOML-compatible parameters (numbers, booleans, strings, arrays, or tables).

## Layout IO inside scripts
- `layout:load_json("tests/fixtures/demo_layout.json", "templates/glove80/keymap.dtsi.j2")` imports JSON with a real template.
- `layout:parse_dts(dts_string)` / `layout:parse_json(json_string, "templates/glove80/keymap.dtsi.j2")` work with raw strings.
- `layout:save_dts("out/keymap.dts")`, `layout:save_dtsi(...)`, `layout:save_json("out/layout.json")`.
- `layout:to_dts_string()` / `layout:to_json_string()` return rendered strings if you want to handle output yourself.
