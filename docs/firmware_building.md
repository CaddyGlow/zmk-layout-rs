# Firmware Build Guide

This crate can compile ZMK firmware directly from layouts (JSON, DTS, or keymap
files) by describing toolchains/keyboards in a manifest and invoking the
`zmk-layout firmware build` CLI workflow.

## Manifest Overview

Manifests live under `firmware_profiles/*.toml` and follow this structure:

```toml
version = 1

[toolchains.zmk]
kind = "zmk_config"
image = "zmkfirmware/zmk-build-arm:stable"
repository = "https://github.com/moergo-sc/zmk"
branch = "v25.05"
[toolchains.zmk.env]
ZMK_CONFIG = "/workspace/config"
[toolchains.zmk.cache]
workspace = "read_write"
build = "read_only"

[keyboards.glove80]
default_toolchain = "zmk"

[[keyboards.glove80.targets]]
id = "left"
board = "glove80_lh"
shield = "glove80_left"
cmake_defs = { CONFIG_ZMK_SPLIT_ROLE = "\"left\"" }
```

- `toolchains.<id>` entries describe container images, repos, branches, and cache
  policies. The `kind` determines which adapter runs (`zmk_config` for west,
  `moergo` for the MoErgo Nix image).
- `keyboards.<id>` map to logical keyboards with a default toolchain and a list
  of build `targets` (board/shield pairs plus optional cmake definitions).
- Targets may provide per-toolchain overrides via
  `[keyboards.<id>.targets.toolchain_overrides.<toolchain>]` to tweak images,
  branches, or extra environment variables for that toolchain only.

See `firmware_profiles/glove80.toml` for a complete, real-world manifest.

## CLI Usage

```
zmk-layout firmware build \
  --manifest firmware_profiles/glove80.toml \
  --keyboard glove80 \
  --toolchain zmk \
  --target left \
  --layout-dts path/to/layout.dts \
  --output dist/glove80-left
```

Key options:

- `--keyboard` / `--toolchain` select entries from the manifest (toolchain is
  optional when a keyboard default exists).
- Layout input is required (`--layout-json`, `--layout-dts`, or `--keymap`
  + `--kconfig`).
- `--target` can be repeated to restrict the build to a subset of target IDs.
- `--env KEY=VALUE` overrides environment variables for the Docker invocation.
- `--disable-cache` skips cache hydration/persist so temporary workspaces are
  always clean.

The CLI always prints the resolved request (keyboard, toolchain, targets,
output path, env overrides) followed by a build summary detailing metadata,
artifacts, log, and build-info paths.

## Caching and Workspace Reuse

ZMK builds require a full west workspace (`.west/`, `app/`, Zephyr modules). To
avoid re-cloning on every run, enable caching in the manifest:

```toml
[toolchains.zmk.cache]
workspace = "read_write"
build = "read_only"
[[toolchains.zmk.cache.extra_paths]]
relative = ".west"
mode = "read_write"
```

- `workspace` controls hydration/persist of `app/` and `config/`.
- `build` persists `build/<target>` outputs for incremental rebuilds.
- Extra paths let you include other workspace-relative folders; caching `.west`
  is essential so `west init` runs only the first time.

Cache data lives under `$ZMK_LAYOUT_CACHE_DIR/firmware/<toolchain>` (defaults to
`~/.cache/zmk-layout`). Pass `--disable-cache` to force a clean workspace.

## Outputs and Logs

Artifacts are copied into the requested `--output` directory. Every successful
build also emits:

- `build-<keyboard>-<toolchain>.log` – live Docker output plus builder notes.
- `build-info-<keyboard>-<toolchain>.json` – structured summary with keyboard,
  toolchain, metadata entries, per-target artifact lists, and duration.

Artifact paths are also recorded inside `build-info.json` so CI or other tools
can consume results without parsing console output. The log and info file names
are derived from manifest IDs (sanitized and lowercased).

## Toolchain Behavior

- **MoErgo (`kind = "moergo"`)** – stages layout/config files into
  `/workspace/config`, runs `build.sh` in the MoErgo image, and copies `.uf2`
  artifacts plus build logs out of `/workspace/artifacts`.
- **ZMK Config (`kind = "zmk_config"`)** – bootstraps a west workspace inside
  the temp directory (`west init/update/zephyr-export`), writes staged keymap
  files, and executes `west build` per target with any manifest-provided cmake
  definitions or shields. Workspace/cache policies control whether `.west/`,
  `app/`, and `build/<target>` directories hydrate from
  `$ZMK_LAYOUT_CACHE_DIR` (or the OS cache directory by default).

Both toolchains stream Docker logs through the CLI and into the shared
`build-*.log` file so failures can be diagnosed even if stdout is truncated.
