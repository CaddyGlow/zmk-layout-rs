# Embedded Profiles

The `profiles/keyboards/` and `profiles/firmwares/` directories are now embedded in the binary at compile time using `rust-embed`. This allows the binary to work without needing the profile directories present at runtime.

## Features

### 1. Embedded at Compile Time
- All `.toml` files in `profiles/keyboards/` and `profiles/firmwares/` are embedded in the binary
- The binary works standalone without requiring these directories at runtime
- Smaller deployment footprint - just distribute the binary

### 2. Filesystem Override
- If profile directories exist at runtime, files are loaded from the filesystem first
- This allows users to override embedded profiles with custom configurations
- Perfect for development and customization workflows

### 3. Loading Methods

#### Keyboard Profiles

```rust
use zmk_layout_rs::profiles::KeyboardProfileDoc;

// Load by name (checks filesystem first, then embedded)
let profile = KeyboardProfileDoc::load("glove80")?;

// Load from specific file path (original behavior)
let profile = KeyboardProfileDoc::from_file("profiles/keyboards/glove80/profile.toml")?;

// List all available profiles (both embedded and filesystem)
let profiles = KeyboardProfileDoc::list_available();
```

#### Firmware Manifests

```rust
use zmk_layout_rs::build::FirmwareManifest;

// Load by name (checks filesystem first, then embedded)
let manifest = FirmwareManifest::load("glove80")?;

// Load from specific file path (original behavior)
let manifest = FirmwareManifest::from_file("profiles/firmwares/glove80.toml")?;

// List all available manifests (both embedded and filesystem)
let manifests = FirmwareManifest::list_available();
```

### 4. Manifest Profile References

In firmware manifests, you can now reference keyboard profiles by name:

```toml
[keyboards.glove80.metadata]
profile = "glove80"  # Loads from embedded or filesystem
```

Or use a path for custom profiles:

```toml
[keyboards.custom.metadata]
profile = "../my-profiles/custom.toml"  # Explicit path
```

### 5. CLI Convenience

The CLI now accepts both full paths and profile names for the `--manifest` argument:

```bash
# Using just the name (loads from embedded or filesystem)
zmk-layout firmware build --manifest glove80 --keyboard glove80 ...
zmk-layout firmware flash --manifest glove80 --keyboard glove80 ...
zmk-layout firmware devices --manifest glove80 --keyboard glove80 ...

# Using full path (original behavior)
zmk-layout firmware build --manifest profiles/firmwares/glove80.toml --keyboard glove80 ...
zmk-layout firmware flash --manifest profiles/firmwares/glove80.toml --keyboard glove80 ...
```

The CLI automatically detects whether you provided a path or a name:
- If the path exists, it loads from that exact file
- If the path doesn't exist and looks like a name (no `/` or `.toml`), it tries loading from embedded profiles
- This maintains backward compatibility while providing convenience

## Search Order

When using `load()` methods:

1. `profiles/{keyboards|firmwares}/{name}.toml` in filesystem (if exists)
2. Embedded `{name}.toml` from compile-time embedded resources
3. Return `NotFound` error if neither exists

## Benefits

- **Single Binary Distribution**: No need to bundle profile directories
- **Development Flexibility**: Override embedded profiles by creating local files
- **Backwards Compatible**: Existing code using `from_file()` continues to work
- **Easy Updates**: Update profiles by rebuilding the binary or providing filesystem overrides
- **Version Control**: Embedded profiles are versioned with the binary

## Implementation Details

- Uses `rust-embed` crate with `#[folder]` attribute
- Embedded at compile time as `&'static [u8]`
- Zero runtime overhead for embedded access
- Supports pattern matching and iteration over embedded files
