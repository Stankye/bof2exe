# bof2exe

Turn a Windows Beacon Object File (BOF) into a standalone command-line tool with named flags, help, defaults, validation, and generated documentation.

The generated executable embeds its object and command schema. It runs without the converter, Rust, or the source files. The project is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).

## Quick start

Keep `bof2exe.exe` and `bof2exe-runner.exe` from the same release together when building tools. Only the generated executable needs distributing.

```powershell
# Draft a schema from a BOF and its CNA command.
./bof2exe.exe init hello.o --cna hello.cna --command hello -o hello.tool.toml
# Edit the parameter names, descriptions, and defaults in hello.tool.toml.
./bof2exe.exe build hello.tool.toml -o hello.exe --docs hello.md
./hello.exe --help
./hello.exe --name Alice --count 3
```

Without CNA metadata, use `init hello.o --format iz` to declare integer and string arguments. Schemas support short options, positionals, choices, numeric switches, and binary input files. Parameters are packed in the BOF's declared order regardless of flag order.

CNA analysis drafts wire types and command metadata. Review the schema before building: script defaults, transformations, validation, and helper functions are not evaluated. See the [standalone CLI guide](docs/standalone-cli.md) for the schema and direct-build options.

## Try the included example

Windows release packages include a ready-to-run greeting tool:

```powershell
./hello.exe --name Alice --count 3
# Hello, Alice! Count: 3
```

To build it from a source checkout, install Rust's Windows x64 MSVC toolchain and Visual Studio C++ Build Tools with the Windows SDK:

```powershell
cargo +stable-x86_64-pc-windows-msvc build --release --locked
./scripts/prepare-fixtures.ps1
./target/release/bof2exe.exe build examples/hello.tool.toml -o hello.exe
./hello.exe --name Alice
```

## Compatibility

Executable generation and native execution support **Windows x64**. Inspection, CNA analysis, argument packing, and object linking also work on other platforms.

The runtime supports standard COFF and BigObj, a documented Beacon API subset, and common Windows compiler output. Check an object with `bof2exe check input.o --json` before conversion. Generated tools execute native code in their own process; use objects you trust.

The pinned TrustedSec validation covers **72 x64 objects statically** and **11 generated CLIs at runtime**. These results do not establish runtime compatibility for every module. See the [validation report](docs/trustedsec-validation.md) and [compatibility matrix](docs/compatibility.md).

## Documentation

The runtime supports [standalone download artifacts](docs/downloads.md) through `BeaconDownload`. Native execution remains subject to the runtime's documented thread and capture limits.

| Topic | Guide |
| --- | --- |
| Schemas, flags, defaults, and generated tools | [Standalone CLIs](docs/standalone-cli.md) |
| Extracting parameter metadata from CNA | [CNA analysis](docs/cna.md) |
| Inspection, execution, manifests, and the Rust API | [Developer tools](docs/developer-tools.md) |
| Combining objects and resources | [Composition](docs/composition.md) |
| Archives, ZIP collections, and COMDAT | [Libraries](docs/libraries.md) |
| Repeatable compatibility and execution checks | [Fixture suites](docs/suites.md) |
| Every command and option | [Generated CLI reference](docs/generated/cli.md) |

For development, see [CONTRIBUTING.md](CONTRIBUTING.md), the [architecture](docs/design.md), and the [release guide](docs/releasing.md). Dependency notices and input-licensing details are in [THIRD_PARTY.md](THIRD_PARTY.md).
