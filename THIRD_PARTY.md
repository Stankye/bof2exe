# Third-party notices and provenance

Project source and repository fixtures are provided under MIT OR Apache-2.0. The parser, linker, runtime, CNA analyzer, and test harness were written for this project. External loader implementations were not incorporated. Public format and API references are listed in the [architecture guide](docs/design.md); ecosystem references are listed in [related projects](docs/references.md).

## Dependencies

`Cargo.lock` records the dependency versions. `deny.toml` defines the accepted dependency licenses, and CI runs `cargo deny check licenses` against that graph. Windows release packages contain `dependencies.json` and the available dependency license and notice files under `licenses/`.

Direct runtime dependencies are `anyhow`, `base64`, `clap`, `crc32fast`, `flate2`, `serde`, `serde_json`, `sha2`, `thiserror`, `toml`, and Windows-only `windows-sys`. Development dependencies are `tempfile` and `find-msvc-tools`.

Review the complete dependency graph before releasing:

```text
cargo metadata --locked --format-version 1
cargo deny check licenses
```

The second command requires the `cargo-deny` developer tool. Packages with alternative licenses are evaluated against the permissive options allowed by `deny.toml`. Unicode data notices must be retained where applicable. License metadata checks supplement review of the actual notices included in a release.

## User-supplied objects

The project's license does not relicense BOFs, CNA files, libraries, or resources supplied to the converter. Their own terms apply when distributing generated tools.

The optional TrustedSec tests use separately obtained, unmodified upstream artifacts at a pinned commit. Upstream source and binaries are excluded from the repository and release packages. The committed validation snapshot contains hashes and test results.
