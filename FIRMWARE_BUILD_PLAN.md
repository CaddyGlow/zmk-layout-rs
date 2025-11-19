# Firmware Build Plan (Simplified)

This rewrite captures only the essentials needed to add firmware builds to
`zmk-layout-rs` while keeping the architecture approachable.

## Objectives
- Compile flashable ZMK firmware directly from layouts/documents managed by this crate.
- Support at least two dockerized toolchains (`zmk_config` + `moergo`) with room to add more.
- Allow callers (CLI or library) to feed JSON, DTS files, or in-memory `DtsDocument`s without unnecessary disk IO.
- Stream Docker logs/progress so users always see live build feedback.

## Toolchains in scope
1. **`zmk_config` / West** – standard Zephyr workflow inside `zmkfirmware/zmk-build-arm`.
2. **`moergo` / Nix** – MoErgo image that runs `build.sh` for Glove80 firmware.

Each toolchain keeps its own configuration (image, repo/branch, command list, extra env).

## Core Architecture (single pass)
1. **Manifest** – a TOML file (`firmware_profiles.toml`) describing toolchains and keyboard profiles.
   - `toolchains.<id>`: `kind`, `image`, `repository`, `branch`, optional cache hints, env overrides.
   - `keyboards.<id>`: default toolchain id, list of build targets (board/shield pairs, cmake defs).
2. **Request preparation** – call-site builds a `BuildRequest` (via the `BuildRequestBuilder`) with layout input, keyboard/toolchain selection, env overrides, and output paths.
3. **Layout staging** – converts JSON/`DtsDocument` into keymap/config files only when the toolchain needs them.
4. **Workspace manager** – creates a temp directory, optionally hydrates caches, and arranges files the toolchain expects (`config/`, `app/`, etc.).
5. **Toolchain adapter** – trait-based (`Toolchain::invoke(ctx, docker, progress)`) with concrete `MoergoToolchain` and `ZmkConfigToolchain` implementations. They know how to prep commands, mount volumes, and parse progress markers.
6. **Docker backend** – a thin trait around CLI execution for now, but isolating it makes swapping to an API client easy later.
7. **Progress/log streaming** – every docker run attaches an output handler that forwards raw lines plus structured checkpoints to a `ProgressReporter` (CLI can render text, GUI can render bars).
8. **Artifact collector** – copies `.uf2`, `.bin`, logs, and writes a `build-info.json` summary.

## Key Types & Symbols

### Manifest & Profile Types
- `FirmwareManifest` (in `build/manifest.rs`)
  - `pub toolchains: HashMap<String, ToolchainProfile>`
  - `pub keyboards: HashMap<String, KeyboardProfile>`
  - `pub version: u32`
- `ToolchainProfile`
  - `pub id: String`
  - `pub kind: ToolchainKind` (`enum ToolchainKind { ZmkConfig, Moergo }`)
  - `pub image: String`
  - `pub repository: Option<String>`
  - `pub branch: Option<String>`
  - `pub env: BTreeMap<String, String>`
  - `pub cache: CachePolicy`
- `CachePolicy`
  - `pub workspace: CacheMode`
  - `pub build: CacheMode`
  - `pub extra_paths: Vec<CachePath>`
- `CacheMode` (`enum CacheMode { Disabled, ReadOnly, ReadWrite }`)
  - `Disabled`: skip cache hydrate/store completely.
  - `ReadOnly`: hydrate from cache if present but never write back.
  - `ReadWrite`: hydrate if available and persist results afterward.
- `CachePath`
  - `pub relative: PathBuf` (workspace-relative path)
  - `pub mode: CacheMode` (defaults to `CachePolicy.workspace` or `CachePolicy.build` depending on which list it belongs to)
- `CachePolicy.workspace` applies to repo/workspace hydration (`west init`, `west update`, MoErgo base files). `CachePolicy.build` applies to compiled outputs (artifact reuse). `extra_paths` entries specify additional workspace-relative folders (e.g., `modules/custom`, `app/boards`) to include in cache snapshots; they honor their own `CacheMode` if set, otherwise inherit from the surrounding context. CLI flag `--no-cache` or `BuildRequest.disable_cache = true` forces both modes to `Disabled` at runtime even if the manifest enables them.
- `KeyboardProfile`
  - `pub id: String`
  - `pub default_toolchain: String`
  - `pub targets: Vec<BuildTarget>`
  - `pub metadata: MetadataMap`
- `BuildTarget`
  - `pub id: String`
  - `pub board: String`
  - `pub shield: Option<String>`
  - `pub cmake_defs: BTreeMap<String, String>`
  - `pub variant: Option<String>`
  - `pub comment: Option<String>`
  - `pub repo_override: Option<String>`
  - `pub branch_override: Option<String>`
  - `pub toolchain_overrides: HashMap<String, ToolchainOverride>` (keyed by concrete toolchain id)
- `ToolchainOverride`
  - `pub image: Option<String>`
  - `pub repository: Option<String>`
  - `pub branch: Option<String>`
  - `pub env: BTreeMap<String, String>`
  - Manifest authors use `toolchain_overrides` to pin repo/branch/image for a specific target+toolchain combination. CLI/API callers pass the target `id` so the builder can resolve these overrides without restating the struct. Override precedence: start with `ToolchainProfile` values, apply `BuildTarget.repo_override/branch_override` if present, then layer the per-toolchain override for the selected toolchain id last (most specific wins). Request-level overrides (e.g., manual repo flag) would then layer on top of all manifest-derived values.

### Build API
- `FirmwareBuilder`
  - Fields: `manifest: FirmwareManifest`, `toolchains: ToolchainRegistry`, `docker: Box<dyn DockerBackend>`
  - Methods: `pub fn new(manifest, docker) -> Self`, `pub fn with_registry(...)`, `pub fn build(&self, request: BuildRequest) -> Result<BuildReport>`, `pub fn builder(&self) -> BuildRequestBuilder`
- `BuildRequest`
  - `pub keyboard_id: String`
  - `pub toolchain_id: Option<String>`
  - `pub targets: Vec<BuildTargetRef>` (`BuildTargetRef { id: String }` referencing `KeyboardProfile.targets[i].id`)
  - `pub layout: LayoutSource`
  - `pub output_dir: PathBuf`
  - `pub extra_env: BTreeMap<String, String>`
  - `pub disable_cache: bool`
  - `pub manifest: Arc<FirmwareManifest>` (shared via `Arc` inside `FirmwareBuilder`; requests clone the `Arc` so they can move across threads without lifetime gymnastics)
  - `pub progress: Box<dyn ProgressReporter + Send>`
- `BuildRequestBuilder`
  - Fluent setters: `keyboard()`, `toolchain()`, `target()`, `layout_json_path()`, `layout_json_value()`, `layout_document()`, `layout_files()`, `output_dir()`, `env()`, `disable_cache()`
  - `pub fn build(self) -> Result<BuildRequest>`
- `BuildReport`
  - `pub success: bool`
  - `pub artifacts: ArtifactReport`
  - `pub logs_path: Option<PathBuf>`
  - `pub metadata: BuildMetadata`
- **Environment precedence**
  - Start with toolchain defaults + Docker-required vars.
  - Merge in manifest-level `ToolchainProfile.env` (later entries override earlier duplicates).
  - Apply target/toolchain override env if defined.
  - Finally merge `BuildRequest.extra_env`, which wins on key collisions; removing a key requires setting it to an empty string explicitly.

### Layout & Workspace
- `LayoutSource` enum
  - `JsonPath(PathBuf)`
  - `JsonValue(serde_json::Value)`
  - `Document(DtsDocument)`
  - `Files { keymap: PathBuf, config: PathBuf }`
- `LayoutStager`
  - `pub fn stage(&self, source: &LayoutSource, workspace: &WorkspaceHandle) -> Result<KeymapArtifacts>`
- `WorkspaceManager`
  - Fields: `root: PathBuf`, `cache: CacheStore`, `fs: FileSystem`
  - Methods: `create_workspace(toolchain_id)`, `hydrate_repo(...)`, `place_layout(...)`, `finalize(...)`

### Toolchain Layer
- `Toolchain` trait
  - `fn kind(&self) -> ToolchainKind`
  - `fn prepare(&self, ctx: &mut BuildContext<'_>) -> Result<(), BuildError>`
  - `fn invoke(&self, ctx: &mut BuildContext<'_>, docker: &dyn DockerBackend, progress: &dyn ProgressReporter) -> Result<BuildOutcome, BuildError>`
  - `fn collect(&self, ctx: &BuildContext<'_>) -> Result<ArtifactReport, BuildError>`
- `ToolchainRegistry`
  - `pub fn register(&mut self, kind: ToolchainKind, factory: ToolchainFactory)`
  - `pub fn create(&self, profile: &ToolchainProfile) -> Result<Box<dyn Toolchain>, BuildError>`
- `BuildContext<'a>`
  - Fields: `workspace: WorkspaceHandle`, `manifest: &'a FirmwareManifest`, `profile: &'a ToolchainProfile`, `request: &'a BuildRequest`

### Docker & Progress
- `DockerBackend` trait
  - `fn ensure_available(&self) -> Result<(), BuildError>`
  - `fn run(&self, invocation: DockerInvocation) -> Result<ProcessStatus, BuildError>`
  - `fn build(&self, opts: DockerBuildOptions) -> Result<(), BuildError>`
- `DockerInvocation`
  - `pub image: String`
  - `pub command: Vec<String>`
  - `pub entrypoint: Option<String>`
  - `pub env: BTreeMap<String, String>`
  - `pub volumes: Vec<VolumeMount>`
  - `pub workdir: Option<PathBuf>`
  - `pub user: Option<DockerUser>`
  - `pub log_handler: Box<dyn OutputHandler>`
- `ProgressReporter` trait
  - `fn log(&self, level: LogLevel, message: impl AsRef<str>)`
  - `fn start_checkpoint(&self, id: &str, message: &str)`
  - `fn complete_checkpoint(&self, id: &str)`
  - `fn fail_checkpoint(&self, id: &str)`
  - `fn update_progress(&self, current: u32, total: u32, status: &str)`

These names are fixed so the implementation can follow the terminology without future churn. Where possible, use borrowing to avoid clones; `Arc` only appears at the `FirmwareBuilder`/`BuildRequest` boundary so validated requests can travel across threads.

## Implementation steps
1. **Phase 1 – Foundations**
   - Manifest parser + validation.
   - `BuildRequest`, `ProgressReporter`, `DockerBackend` traits with a default CLI implementation.
   - Skeleton CLI command (`zmk-layout firmware build ...`) that loads a manifest and echoes the resolved request.
   - Update `CHANGELOG.md` under a new section describing the manifest + trait scaffolding work, then commit.
2. **Phase 2 – MoErgo Toolchain (simpler)**
   - Implement layout staging + workspace manager for MoErgo.
   - Run the Docker image with streaming logs; collect artifacts.
   - Add tests using a fake Docker backend.
   - Append a `CHANGELOG.md` entry covering the MoErgo toolchain and stage the commit for the phase.
3. **Phase 3 – ZMK Config Toolchain**
   - Extend workspace manager with `west` bootstrap + optional caches (repo checkout, build outputs).
   - Support build matrices (boards/shields) and per-board progress.
   - Provide cache toggles via manifest and CLI flags.
   - Update `CHANGELOG.md` summarizing the `zmk_config` support before committing.
4. **Phase 4 – Polish & Testing**
   - Structured progress events, manifest documentation, integration tests that run when Docker is available.
   - Hook library API (`FirmwareBuilder::build(request)`) to the same pipeline.
   - Final `CHANGELOG.md` update documenting progress/reporting/polish and commit to close the milestone.

## Testing approach
- Unit tests for manifest parsing, docker invocation assembly, and cache key derivation.
- Fake Docker backend to validate toolchain command generation without touching Docker.
- Optional integration tests (behind `DOCKER_AVAILABLE=1`) to run small end-to-end builds.

## Open considerations
- Whether flashing commands live in this crate or a follow-up project.
- Exact cache layout (global vs per-project) once we have real workloads.
- Potential switch to a Docker client library if/when CLI invocation becomes limiting.

## End-to-end build flow
1. CLI loads the manifest from `firmware_profiles.toml` (default search path: cwd, repo root, explicit `--manifest`). Deserialization validates toolchain ids, ensures keyboard targets reference existing toolchains, and normalizes cache policies (apply derived defaults when omitted).
2. `FirmwareBuilder::builder()` populates a `BuildRequestBuilder` with the manifest `Arc`, the default keyboard/toolchain pairing, and the selected layout source. CLI flags or library callers may override the keyboard, toolchain, or target list before calling `.build()`.
3. Builder resolves targets: pick explicit `BuildTargetRef`s when provided, otherwise expand every target tied to the chosen keyboard. Each target inherits toolchain overrides, env vars, cmake defs, and repo/branch hints as described earlier.
4. Workspace manager allocates a temp directory such as `<tmp>/zmk-layout-<timestamp>/<toolchain>/<target-id>`. The manager hydrates workspace caches (repo checkouts, base files) when enabled and records everything in a manifest file stored under the workspace root for debugging.
5. Layout stager materializes any JSON or `DtsDocument` sources into the workspace: generated keymap and config files land where each toolchain expects them (`config/boards/*` for MoErgo, `config/<shield>.conf` + `boards/<board>.keymap` for West). If callers already passed file paths the stager simply copies or hard-links them into place.
6. Toolchain `prepare()` hook tweaks the workspace (e.g., MoErgo writes `build-vars.mk`, ZMK config ensures `west.yml` modules exist). Once ready, `invoke()` constructs a `DockerInvocation` with proper mounts (`workspace:/workdir`, optional caches under `$HOME/.cache/zmk-layout/…`), env, and commands.
7. Docker backend runs the container while streaming stdout/stderr into a `ProgressReporter`. Checkpoints include `manifest/load`, `layout/stage`, `toolchain/prepare`, `docker/run`, and `artifacts/collect`. Toolchain-specific progress (per target compile, `west build` percent) flows through `update_progress`.
8. When the container exits, the toolchain `collect()` hook finds produced artifacts, copies them into `<output>/<keyboard>/<target-id>/`, writes `build-info.json` (metadata, git hash, command array, duration), and persists log files. Cache store snapshots updated files when `CacheMode::ReadWrite`.

## Toolchain details

### MoErgo (Nix image)
- Workspace layout mirrors their reference repo: `workspace/app/` contains keyboard sources, `workspace/keymap/` stores generated config, and `workspace/build.sh` is provided by the container.
- Required inputs: layout keymap (JSON or DTS converted to `keymap.dtsi`), optional `config.h`, and MoErgo-specific metadata (profile name, variant). `ToolchainProfile.metadata` stores defaults such as `variant = "default"` to avoid manual flags.
- `prepare()` writes a `build.env` file with env overrides, ensures `nix.conf` matches the requested channel, and downloads pinned sources if caches are disabled. Cache hydration primarily copies `nix-store` derivations into a shared volume so repeated builds skip fetches.
- `invoke()` executes `["/bin/bash", "-lc", "./build.sh --board ${board} --shield ${shield:-glove80}"]`. For boards without shields the flag is omitted. Logs are raw `nix build` output, and `ProgressReporter` converts known MoErgo markers (e.g., `building '${drv}'`) into checkpoints.
- `collect()` grabs `firmware/*.uf2`, `logs/*.txt`, and `manifest.json` from the container workspace. These artifacts land beneath `output/<keyboard>/<target>/moergo/` to keep separation from other toolchains.

### ZMK Config (west + Zephyr)
- Workspace houses a shallow clone of `zmk_config` (`workspace/app/`), the Zephyr fork (if `repository` differs), and modules defined in `west.yml`. Cache hydration seeds `.west/` and `build/<board>` directories to accelerate incremental builds.
- `prepare()` ensures `west init` and `west update` run once per workspace. It writes staged keymap/config files into `config/` relative to `app/` following ZMK conventions, generates `build-targets.json` describing the list of board/shield pairs, and patches `CMakeLists.txt` if manifest metadata injects extra settings.
- `invoke()` typically calls `west build -s app -b <board> -- -DZMK_CONFIG=/workspace/config ...`. Targets that specify `shield` append `-DSHIELD=<shield>`. Optional manifest flags such as `cmake_defs` become additional `-DKEY=VALUE`. When the manifest indicates a repo/branch override, the workspace manager ensures the `west` manifest uses that remote before invocation.
- Progress reporting hooks into `west` output: parse `[n/%]` markers, surface compile errors immediately, and wrap each target build with `start_checkpoint("target::<id>")`.
- `collect()` copies build artifacts (`build/zephyr/zmk.uf2`, `build/zephyr/zephyr.hex`, `build.log`), plus `west.meta.json` (a summary containing git revisions, cmake cache, and target info). Artifacts live alongside MoErgo results but include the toolchain id and board/shield in their directory name for clarity.

## CLI & configuration defaults
- Primary entry point: `zmk-layout firmware build --keyboard glove80 --target left --layout layout.json --output ./dist`. CLI infers manifest path (search order mentioned earlier) and exposes flags `--toolchain`, `--target` (repeatable), `--list-targets`, `--list-toolchains`.
- Global flags: `--no-cache`, `--progress plain|json|auto`, `--docker-binary` (default `docker`), `--manifest /path/to/file`, `--workspace /tmp/foo` (optional override for debugging).
- CLI always writes a short `build-summary.json` near the output directory capturing request info, success/failure, and artifact pointers, enabling wrappers or CI steps to parse results without scanning stdout.
- Library callers may skip the CLI entirely by instantiating `FirmwareBuilder` with a manifest `Arc` and injecting a fake `ProgressReporter` and `DockerBackend` for testing or remote execution.

## Structured progress semantics
- All progress events share a `context_id` built from `<keyboard>::<toolchain>::<target-id>` so concurrent builds can multiplex output cleanly.
- Baseline checkpoints: `manifest/load`, `request/resolve`, `workspace/create`, `workspace/cache`, `layout/stage`, `toolchain/prepare`, `docker/run`, `artifacts/collect`, `cache/store`. Each checkpoint emits `start`, `complete`, `fail` transitions.
- `update_progress(current, total, status)` is used sparingly: MoErgo surfaces the number of derivations built, while ZMK config uses `west`’s native percentage. CLI adapters can convert these events into text spinners or JSON payloads for machine consumption.
- Raw docker logs still flow through `log(level, message)` ensuring users can inspect verbose compiler output even if structured events misbehave.

## Definition of done (initial shipping criteria)
- Phase 2 (MoErgo) completes when a Glove80 layout JSON builds end-to-end via CLI with streamed logs, artifacts written to disk, and unit tests covering manifest parsing plus docker invocation assembly.
- Phase 3 (ZMK config) is complete when at least one reference board/shield pair compiles via the new pipeline, caches can be toggled from CLI, and fake docker tests assert `west` command assembly (boards, shields, `cmake_defs`).
- Phase 4 finishes once progress events cover every major step, documentation for manifest/toolchain usage exists under `docs/firmware-build.md`, and CI optionally runs integration tests when docker is available.
