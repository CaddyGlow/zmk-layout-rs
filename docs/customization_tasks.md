# Layout Customization Tasks & CLI

The layout task format lets you describe repeatable modifications for a base Devicetree keymap. A task
file stays under version control, so you can reapply it whenever the upstream template changes. This
document summarizes the schema, conflict policies, and the `zmk-layout` CLI that ships with this crate.

## File Structure

Task files are TOML documents with three main sections:

```toml
[base]
template = "glove80-community"
version = "1.2.0"          # documentation only

[config]
format_version = "1.0.0"
default_conflict = "prompt" # prompt | override | skip | script
comment = "My layout tweaks"

[[tasks]]
id = "swap-tab"
type = "override"
path = "layers.base.bindings[5]"
value = "&kp ESC"
target = "layers.base.bindings[5]"
comment = "Move ESC onto TAB"
```

- `[base]` is informational metadata so humans remember which template/version the plan expects.
  The CLI always requires the actual base layout via `--base-layout`, so these fields are warnings only.
- `[config]` stores the schema format version, default conflict policy, optional global comment, and (later)
  a path to a Lua conflict script. Every file must set `format_version`.
- `[[tasks]]` entries describe ordered operations. Each task needs a unique `target` string that describes
  which structure it owns (e.g., `layers.base.bindings[0]`, `combos.my_combo`). Targets power conflict
  detection so overlapping edits are obvious.

### Supported Task Types

| Type          | Required Fields                                                         | Target Convention             | Notes |
|---------------|-------------------------------------------------------------------------|--------------------------------|-------|
| `override`    | `path`, `value`, optional `from`                                        | matches `path`                | Replaces a single binding slot. `from` (or top-level `expected`) guards against drift. |
| `combo`       | `name`, `key_positions`, `binding`; optional `timeout_ms`, `layers`, `conditions` | `combos.<name>`               | `layers` accepts numeric indexes or layer names. `conditions` is a list of strings (e.g., `layer_state == nav`) stored alongside the combo. |
| `layer`       | `name`, `bindings`; optional `metadata` map                              | `layers.<name>`               | Metadata entries (color, label, etc.) are written into properties. |
| `layer-order` | `layer` plus `position` *or* `before`/`after`                            | `layers.order.<layer>`        | Reorders the layer list in the keymap node. |
| `behavior`    | `behavior`, `settings` map                                              | `behaviors.<name>`            | Updates behavior bindings and properties (`bindings` array is optional; timing/label fields are merged). |
| `meta`        | `key`, `value`                                                          | `meta.<key>`                  | Stores metadata extras on the layout (consumed by adapters/templates and surfaced in task summaries). |
| `script`      | `filename` or `script`, optional `args`                                 | `scripts.<identifier>`        | Executes Lua automation with the same helpers exposed by `zmk-layout keymap lua`. |

### Conflict Policies & `expected`

Each task inherits `config.default_conflict` and may override it per entry. Policies:

- `prompt` (default): report a conflict and stop the run.
- `override`: log the mismatch and keep going.
- `skip`: log the mismatch and ignore the task.
- `script`: invoke `config.conflict_script` (Lua) to decide whether to override, skip, or abort.

To guard against upstream changes, provide either `from = "&kp Q"` (override tasks) or the generic
`expected = "layers base order"` field. When the actual layout does not match `expected`, the per-task
conflict policy controls whether the run aborts, skips, or overrides.

## CLI Quickstart

The `zmk-layout` binary ships with this crate. It consumes a base layout and task file to perform
three core actions:

```bash
# Apply tasks and write to a new DTS
zmk-layout keymap apply \
  --tasks tests/fixtures/tasks_regression_config.toml \
  --base-layout tests/fixtures/tasks_regression_base.dts \
  --output out/tasks_regression_base.generated.dts

# Check what would happen without touching the file
zmk-layout keymap validate \
  --tasks tests/fixtures/tasks_regression_config.toml \
  --base-layout tests/fixtures/tasks_regression_base.dts

# Preview changes as a unified diff
zmk-layout keymap diff \
  --tasks tests/fixtures/tasks_regression_config.toml \
  --base-layout tests/fixtures/tasks_regression_base.dts
```

Helpful flags:

- `--conflicts override|skip|prompt|script`: temporarily override the default conflict policy.
- `--base-template NAME` / `--base-version VERSION`: annotate which template you believe the task file
  targets. If the file's `[base]` section disagrees, the CLI prints a warning.
- `--combo-conditions`: after `apply`, `validate`, or `diff`, print a summary of combo tasks that declare `conditions`. Each entry lists the combo name, target, and condition string(s).

The commands stream per-task status lines, e.g. `APPLIED`, `SKIPPED`, or `CONFLICT`, along with any
messages supplied by the engine. `diff` mode also prints a unified diff between the input DTS and the
transformed document so you can review the patch before saving.

## Target Naming Tips

Targets should reflect the logical ownership of a task so conflicts remain obvious:

- Overrides should reuse their `path` (e.g., `layers.base.bindings[4]`).
- Layer tasks own the entire layer (`layers.nav`).
- Combos map to `combos.<name>`.
- Layer order operations should reference `layers.order.<layer>`.

When in doubt, choose a unique string under the same prefix as the structure you mutate. Deterministic
targets ensure two people editing the same slot are prompted to resolve the clash.

## Troubleshooting

- **"failed to parse layout"** – ensure the `--base-layout` DTS compiles on its own (includes resolved,
  macros available, etc.).
- **Immediate conflicts** – inspect the `before => after` snippets printed for each task or add an
  `expected = "..."` string when the task should bail if the base layout drifted.
- **Skipped behavior/meta/script tasks** – these tasks execute now; they only skip when the settings
  map is empty or a conflict policy tells the engine to skip a mismatched `expected` block.

## Combo Conditions

Combos can restrict when they fire by declaring a `conditions = ["..."]` array inside the task. During
execution, the engine renders these conditions into the generated DTS as tagged comments prefixed with
`// zmk-task:condition`. The prefix keeps task-managed conditions distinct from normal comments while
also ensuring `zmk-layout keymap diff` highlights them as part of the patch. Example:

```toml
[[tasks]]
type = "combo"
name = "combo_demo"
key_positions = [0, 1]
binding = "&kp TAB"
conditions = ["layer_state == base", "mods.shift"]
target = "combos.combo_demo"
```

Running `zmk-layout keymap apply --combo-conditions ...` prints:

```
combo conditions:
  - combo_demo (combos.combo_demo) :: layer_state == base, mods.shift
```

and the output DTS receives:

```dts
// zmk-task:condition layer_state == base
// zmk-task:condition mods.shift
combo_demo {
    ...
};
```

These comments are idempotent — editing the task file and reapplying updates the comment block without
duplicated lines — and they are included in `TaskOutcome` logs (“combo conditions: …”) so plain runs
still surface the data.

## Lua Scripting

`script` tasks are now powered by an embedded Lua engine. Scripts can either be inline
(`script = """ ... """`) or reference a `.lua` file relative to the task file. The fluent `layout`
global (1-based indices) mirrors the declarative task surface:

- `layout:layer(name)` – stage binding/metadata edits with `:bind`, `:bindings`, `:meta`, `:apply`.
- `layout:combo(name)` – configure combos with `:keys`, `:binding`, `:timeout`, `:on_layers`, `:when`, `:apply`.
- `layout:behavior(name)` – update bindings/params with `:bindings`, `:param`, `:apply`.
- `layout:move_layer(name, index)` / `layout:remove_layer(name)` – reorder or drop layers.
- `layout:meta(key, value)` – write layout-level metadata extras.
- Queries: `layout:get_layer`, `layout:get_combo`, `layout:get_behavior`, `layout:list_layers`, `layout:list_combos`, `layout:list_behaviors`.
- I/O: `layout:load_dts/dtsi/json`, `layout:parse_dts/parse_json`, `layout:save_dts/save_json`, `layout:to_dts_string/to_json_string`.
- `log(message)` appends notes; globals `ARGS`, `TASK_ID`, `TARGET`, and `COMMENT` are populated during task execution.

Scripts run against a clone of the layout; the mutated document is written back only when the run
succeeds and the CLI is in apply mode. `validate` still executes the script so logs/errors surface,
but the resulting layout is discarded.

When `config.conflict_script` is set and a task uses `conflict = "script"`, conflicts call into the
referenced Lua file. Define a `resolve(conflict)` function that returns a map with an `action`
(`"override"`, `"skip"`, or `"abort"`) and an optional `message`. Example:

```lua
function resolve(conflict)
    if string.find(conflict.reason, "layers.base") then
        return { action = "override", message = "trusted override" }
    end
    return { action = "abort", message = "needs manual review" }
end
```
