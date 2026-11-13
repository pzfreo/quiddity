# Using quiddity from Rust

quiddity is two crates on crates.io:

- **`quiddity`**: feature recognition, revision comparison, evidence and the command line.
- **`haecceity`**: the geometry underneath. It reads STEP into a B-rep, answers queries on it, and reads and writes AP242 PMI. It reads, queries and writes B-reps; it does not model them.

```toml
[dependencies]
quiddity = "0.5"
```

The [Rust API evolution](rust-api.md) page says which types may grow, and how to match and build them so that a minor release won't break your code.

## Cargo features

| Feature | Crate | Default | What it adds | Pulls in |
|---|---|---|---|---|
| `presentation` | both | on | drawing PMI callouts (frames, dimensions, datums, notes) as geometry | nothing |
| `cli` | quiddity | on | the `quiddity` command line and its shell completion; turns on `manifest` | clap, clap_complete, tempfile |
| `manifest` | both | on (through `cli` in quiddity) | scanning a large STEP assembly and reading one product at a time ([assembly manifests](assemblies.md)) | serde, serde_json, sha2, tempfile, memmap2 |
| `json` | both | off | serde serialisation of the documents | serde, serde_json |
| `json-import` | both | off | the versioned PMI JSON interchange (`quiddity pmi json …`, [reference](pmi-json.md)); turns on `json` | serde, serde_json |
| `f3d` | both | off | Autodesk Fusion `.f3d` input: visible solid geometry only, no PMI ([Fusion input](f3d.md)) | cadmpeg codecs |

## A library-only build

Without the command line, clap stays out of your build:

```toml
quiddity = { version = "0.5", default-features = false, features = ["presentation"] }
```

## WebAssembly

The library builds for `wasm32-unknown-unknown` without the command line or the assembly scan, which needs a file system and memory mapping. This is how draftwright's browser editor uses it, and CI builds it on every change:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p quiddity --lib --release --target wasm32-unknown-unknown \
  --no-default-features --features presentation,json
```

Read STEP from bytes in the browser (`haecceity` reads a `Document` from a byte slice); there is no file system there.
