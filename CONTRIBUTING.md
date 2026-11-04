# Contributing

Start with the [development guide](docs/development.md) for setup, checks, and the source layout. The [architecture guide](docs/design.md) explains the conversion and runtime paths.

Keep changes focused on converting BOFs into usable standalone CLIs. Loader, linker, and CNA changes should explain which input or conversion behavior they support. Update the relevant user documentation and regenerate the command reference when CLI help changes.

For a bug report, include the project version, operating system, compiler/toolchain, command, expected result, and actual error. For object compatibility issues, include compiler flags and a small reproducer when its license permits sharing. Redact sensitive paths and output.

Add regression coverage for behavior changes. Runtime changes should use small source fixtures with clear expected output. External BOF collections remain opt-in inputs; commit their provenance and result summaries rather than their source or binaries.

Contributions use the project's MIT OR Apache-2.0 license. Retain required notices for any third-party material and describe its origin.
