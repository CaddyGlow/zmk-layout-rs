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
  a path to a Rhai conflict script. Every file must set `format_version`.
- `[[tasks]]` entries describe ordered operations. Each task needs a unique `target` string that describes
  which structure it owns (e.g., `layers.base.bindings[0]`, `combos.my_combo`). Targets power conflict
  detection so overlapping edits are obvious.

### Supported Task Types

| Type          | Required Fields                                                         | Target Convention             | Notes |
|---------------|-------------------------------------------------------------------------|--------------------------------|-------|
| `override`    | `path`, `value`, optional `from`                                        | matches `path`                | Replaces a single binding slot. `from` (or top-level `expected`) guards against drift. |
| `combo`       | `name`, `key_positions`, `binding`; optional `timeout_ms`, `layers`      | `combos.<name>`               | `layers` accepts numeric indexes or layer names. |
| `layer`       | `name`, `bindings`; optional `metadata` map                              | `layers.<name>`               | Metadata entries (color, label, etc.) are written into properties. |
| `layer-order` | `layer` plus `position` *or* `before`/`after`                            | `layers.order.<layer>`        | Reorders the layer list in the keymap node. |
| `behavior`    | `behavior`, `settings` map                                              | `behaviors.<name>`            | Parsed but currently skipped until behavior editing lands. |
| `meta`        | `key`, `value`                                                          | `meta.<key>`                  | Reserved for future metadata injection. |
| `script`      | `filename` or `script`, optional `args`                                 | `scripts.<identifier>`        | Deferred until the Rhai phase ships. |

### Conflict Policies & `expected`

Each task inherits `config.default_conflict` and may override it per entry. Policies:

- `prompt` (default): report a conflict and stop the run.
- `override`: log the mismatch and keep going.
- `skip`: log the mismatch and ignore the task.
- `script`: reserved for Rhai automation (no-op today).

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
