# Changelog

## 0.4.1

- Resolve conventional unqualified Windows loader imports for direct calls and import pointers.
- Recognize CNA commands with null or omitted inline argument buffers as no-argument signatures.
- Add opt-in validation against a pinned TrustedSec corpus: 72 x64 objects checked statically and eleven generated CLIs tested at runtime.
- Reorganize user and developer documentation and add repository publication checks.

## 0.4.0

- Add `init` and `build` for converting BOFs into standalone CLIs from editable tool schemas or CNA metadata.
- Support named and short flags, positionals, defaults, choices, numeric switches, and binary file inputs.
- Generate command-specific help, version output, and Markdown documentation.
- Add a smaller runner template for generated executables.

## 0.3.0

- Add COFF archive and ZIP object-library inputs, lazy member selection, and COMDAT handling.
- Add declarative fixture suites and native compiler/library tests.

## 0.2.0

- Add static CNA signature discovery and typed format packing.
- Add object/resource composition, BigObj support, common and weak symbols, compiler helpers, and unwind registration.

## 0.1.0

- Add COFF inspection, compatibility checks, typed arguments, Windows x64 execution, and raw executable bundles.
