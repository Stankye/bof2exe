# Architecture

`bof2exe` converts an object and a command schema into a standalone Windows CLI. The loader, linker, and analysis commands support preparing and diagnosing objects for that workflow.

## Conversion

`init` creates an editable TOML schema. CNA analysis can supply command metadata and an argument format; users review names, defaults, and descriptions. `tool` validates the schema and uses one `clap` model for parsing, help, and generated command documentation.

`build` checks the object and appends a versioned payload to `bof2exe-runner.exe`. The payload contains the object, entrypoint, and command definition. An integrity hash detects accidental payload changes. Startup reads the embedded definition, parses arguments, packs them in wire order, and invokes the object. Building a tool does not require recompiling Rust.

The converter falls back to its own executable image if the sibling runner is absent. Both templates dispatch embedded commands before considering converter arguments. Generated tools execute in their own process and do not inherit the `run` command's worker deadline.

## Modules

| Module | Responsibility |
| --- | --- |
| `tool`, `standalone`, `bundle` | Schemas, generated CLI startup, and executable payloads |
| `args`, `manifest`, `cna` | Wire values, execution defaults, and static script metadata |
| `coff`, `inspect` | Checked object parsing and portable compatibility reports |
| `link`, `library`, `comdat` | Object preparation, resources, library selection, and section selection |
| `runtime` | Native Windows x64 execution and Beacon callbacks |
| `suite` | Compatibility expectations and callback assertions |
| `src/main.rs`, `src/cli.rs`, `src/generate.rs` | Converter commands, worker orchestration, and reference generation |

## Native execution

The runtime uses the same compatibility checks as `check`. It lays sections out on separate 4 KiB pages in a bounded allocation, applies relocations while memory is writable, sets final protections, and flushes the instruction cache. Import slots and nearby jump stubs handle references whose final function addresses are outside a signed REL32 displacement.

Imports resolve through the supported Beacon functions, compiler/CRT helpers, four conventional Windows loader functions, or explicit `DLL$Export` names. `__imp_` symbols use import pointers. Host-side implicit DLL lookup uses System32; object code that calls Windows loader functions controls its own later loads. Referenced unknown symbols fail compatibility checks.

AMD64 version-1 unwind tables are validated and registered for the duration of the call. Module handles, function tables, and mappings are released afterward. The native lifetime contract requires the object to finish synchronously; the runtime does not initialize TLS, static constructors, or a C++ language runtime.

`run` starts a worker process, sends an object and packed arguments over stdin, captures callback bytes, and enforces a deadline. The Rust API invokes native code in the caller's process. Both execution paths require trusted input code. See the public API's safety contract and the [compatibility matrix](compatibility.md).

## Parsing and linking

CNA analysis records source expressions and recognizes a conservative set of literal signatures. It does not evaluate includes, helpers, control flow, or transformations. Empty inline argument buffers can establish a no-argument signature. See [CNA analysis](cna.md).

The partial linker accepts COFF/BigObj, remaps relocations, resolves strong/common/weak symbols, and writes deterministic standard COFF. Resources become read-only sections with data and length symbols. Common symbols, weak aliases, and COMDAT sections are prepared before execution.

Archives and ZIP collections are read in memory. Library selection repeatedly finds providers for live references after COMDAT selection. Whole-library inclusion is explicit. See [composition](composition.md), [libraries](libraries.md), and [suites](suites.md).

## References

- [Microsoft PE/COFF specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format): file records and relocation semantics.
- [Microsoft x64 calling convention](https://learn.microsoft.com/en-us/cpp/build/x64-calling-convention): register arguments, stack slots, and variadic calls.
- [Fortra BOF C API](https://hstechdocs.helpsystems.com/manuals/cobaltstrike/current/userguide/content/topics/beacon-object-files_bof-c-api.htm): Beacon function contracts.
- [Related projects](references.md): background and ecosystem references.
