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
| `behavior`    | `behavior`, `settings` map                                              | `behaviors.<name>`            | Updates behavior properties (`bindings`, timing fields, labels, etc.). |
| `meta`        | `key`, `value`                                                          | `meta.<key>`                  | Stores arbitrary metadata under a top-level `meta { key = value; }` block for documentation/export tooling. |
| `script`      | `filename` or `script`, optional `args`                                 | `scripts.<identifier>`        | Executes Lua automation with access to the same layout engine used by declarative tasks. |

### Conflict Policies & `expected`

Each task inherits `config.default_conflict` and may override it per entry. Policies:

- `prompt` (default): report a conflict and stop the run.
- `override`: log the mismatch and keep going.
- `skip`: log the mismatch and ignore the task.
- `script`: reserved for Lua automation (no-op today).

To guard against upstream changes, provide either `from = "&kp Q"` (override tasks) or the generic
`expected = "layers base order"` field. When the actual layout does not match `expected`, the per-task
conflict policy controls whether the run aborts, skips, or overrides.

## CLI Quickstart

The `zmk-layout` binary ships with this crate. It consumes a base layout and task file to perform
three core actions:

```bash
# Apply tasks and write to a new DTS
zmk-layout apply --tasks layout_tasks.toml --base-layout keymap.dts --output keymap.generated.dts

# Check what would happen without touching the file
zmk-layout validate --tasks layout_tasks.toml --base-layout keymap.dts

# Preview changes as a unified diff
zmk-layout diff --tasks layout_tasks.toml --base-layout keymap.dts
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
- **Silent skips** – behavior/meta/script tasks are parsed today but treated as future work, so the CLI
  will report them as `SKIPPED`.

For additional background, see `LAYOUT_TASK_PLAN.md` for the full roadmap.

## Combo Conditions

Combos can restrict when they fire by declaring a `conditions = ["..."]` array inside the task. During
execution, the engine renders these conditions into the generated DTS as tagged comments prefixed with
`// zmk-task:condition`. The prefix keeps task-managed conditions distinct from normal comments while
also ensuring `zmk-layout diff` highlights them as part of the patch. Example:

```toml
[[tasks]]
type = "combo"
name = "combo_demo"
key_positions = [0, 1]
binding = "&kp TAB"
conditions = ["layer_state == base", "mods.shift"]
target = "combos.combo_demo"
```

Running `zmk-layout apply --combo-conditions ...` prints:

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
(`script = """ ... """`) or reference a `.lua` file relative to the task file. Each script
receives a helper API roughly equivalent to the declarative task set:

- `set_binding(layer: string, index: int, binding: string)` – replace a single binding (`override`).
- `set_layer(layer: string, bindings: array<string>)` – replace the entire binding list (`layer`).
- `set_layer_metadata(layer: string, metadata: map)` – write layer metadata entries.
- `move_layer(layer: string, index: int)` – reorder a layer to an absolute index (`layer-order`).
- `upsert_combo(name: string, positions: array<int>, binding: string)` – basic combo creation.
- `upsert_combo_full(name, positions, binding, timeout_ms_or_unit, layers: array<int|string>, conditions: array<string>)` – full combo editing, including timeout overrides, layer masks, and condition strings.
- `set_behavior_bindings(name: string, bindings: array<string>)` – replace a behavior’s bindings.
- `set_behavior_settings(name: string, settings: map)` – update behavior properties (tapping term, labels, etc.).
- `set_meta(key: string, value: any)` – add/update entries in the `meta { ... }` block.
- `log(message: string)` – append notes to the task outcome.
- `ARGS` – a map built from the task’s `args = { ... }` table, exposed as a global variable.

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
