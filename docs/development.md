# Development

## Setup

Use a current stable Rust toolchain. Windows native tests and executable generation require `stable-x86_64-pc-windows-msvc`, Visual Studio C++ Build Tools, and the Windows SDK. The repository's Cargo configuration statically links the MSVC runtime. Native compiler tests discover the installed tools through `find-msvc-tools`.

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc --component rustfmt --component clippy
cargo +stable-x86_64-pc-windows-msvc build --locked
```

Linux and other hosts can build the converter and run portable tests with their stable host toolchain. Native Windows tests are selected with target configuration. The external TrustedSec tests are ignored unless explicitly selected.

## Checks

From the repository root, with the intended toolchain selected:

```text
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo doc --no-deps --locked
cargo run --locked -- docs --output docs/generated
```

On Windows with a different default toolchain, add `+stable-x86_64-pc-windows-msvc` after `cargo`. CI runs these checks on Linux and Windows, checks the generated reference for changes, and checks dependency licenses separately.

Run `./scripts/check-repository.ps1` to validate local documentation links, publication files, PowerShell syntax, and common accidental credential or workstation-path references. It works with or without a Git checkout and requires PowerShell 5.1 or newer.

## Fixtures

`./scripts/prepare-fixtures.ps1` compiles the source fixtures and creates the helper ZIP under `target/fixtures`. It explicitly uses the Windows x64 MSVC Rust toolchain. The [composition](composition.md) and [suite](suites.md) examples use these files. Native tests also compile their own temporary fixtures, including C++ COMDAT and `.lib` examples.

If Windows blocks a script under the local execution policy, invoke it for that process with `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/prepare-fixtures.ps1`. This does not change the system policy.

BOF fixture files are compiled directly by `rustc`; they are not Cargo targets. Keep their source formatted with `rustfmt`. Test expectations should check observable results, including argument bytes, callback kinds, output, exit status, or failure diagnostics.

The [TrustedSec validation guide](trustedsec-validation.md) describes the opt-in upstream tests and pinned inputs. Ordinary checks do not download or execute external collections.

## Source layout

| Path | Contents |
| --- | --- |
| `src/` | Public library, converter, and standalone runner |
| `tests/` | Portable and native integration tests |
| `tests/fixtures/` | Source fixtures compiled during tests or example preparation |
| `include/` | Supported Beacon API declarations for C inputs |
| `examples/` | Schemas, manifests, composition specs, and fixture suites |
| `docs/` | User and developer guides, generated reference, and validation snapshots |
| `scripts/` | Fixture preparation, validation, and packaging |
| `target/`, `dist/` | Ignored build inputs, local corpora, reports, and release artifacts |

`Cargo.lock` is committed for reproducible dependency selection. Keep generated objects, executables, downloaded corpora, editor settings, and local credentials out of source changes. Release preparation is documented in [releasing.md](releasing.md).
