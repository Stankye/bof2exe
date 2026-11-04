# Object composition

The linker emits ordinary relocatable COFF from an ordered set of local object files. It is portable: linking/inspection can run without executing a single native instruction from the inputs. Execution remains a separate Windows x64 step.

## Complete example

From the repository root on Windows x64, after building the converter and adding it to `PATH`:

```powershell
./scripts/prepare-fixtures.ps1
bof2exe link --spec examples/compose.toml --plan --json
bof2exe link --spec examples/compose.toml -o composed.o --json
bof2exe run composed.o
bof2exe build composed.o -o composed.exe
./composed.exe
```

`link_main` calls `helper` from another object and prints the resource stored in `examples/message.txt`. The executable carries the resulting object and resource; those source files are not needed at runtime.

## Spec

The fixture preparation script also creates a deflated helper library. The equivalent library-based build is:

```powershell
./scripts/prepare-fixtures.ps1
bof2exe link --spec examples/compose-library.toml -o from-library.o --json
bof2exe run from-library.o
```

Release packages include precompiled fixtures and the ZIP library. Run the composition commands from the release directory without the preparation step to use them without a compiler.

```toml
schema = 1
name = "composed-example"
entry = "go"
objects = ["main.o", "helper.o"]

[[resources]]
path = "message.txt"
symbol = "greeting"
```

Paths resolve beside the spec. Unknown fields/schema versions are errors. `--entry` overrides the spec entrypoint. `--plan` computes the result and its runtime compatibility without writing an output. A partial link may succeed while reporting runtime compatibility issues, because retaining unresolved externals is a valid COFF operation. `check` remains the command that fails for runtime incompatibility.

## Resolution rules

| Input condition | Result |
| --- | --- |
| External reference matches a strong definition | Reference redirects to the defining object |
| Common declarations with one name | Allocate the largest size in zero-initialized storage, aligned to 16 bytes |
| Strong definition plus common declarations | Strong definition wins |
| Weak alias with no strong definition | Follow its fallback, detecting invalid indices and cycles |
| Weak NOLIBRARY/ALIAS search mode | Supported without archive acquisition |
| Weak LIBRARY search mode | Search explicitly supplied libraries before following the fallback |
| Weak anti-dependency mode | Rejected |
| Duplicate strong definitions | Rejected unless handled by their declared COMDAT selection |
| Unresolved external | Retained and listed in the link report |
| Mixed input architectures | Rejected |

Local/static symbols remain object-specific. Each relocation is remapped to the final symbol table. COMDAT roots and associative sections are selected before flattening, and references to discarded external definitions redirect to the winner. Grouping flags and auxiliary records are normalized afterward. Debug auxiliary fidelity and general identical-code folding are not provided. See [library and COMDAT rules](libraries.md) for selection modes and conservative error cases.

Each resource adds a read-only section and two external definitions: `greeting` identifies the bytes and `greeting_len` identifies a four-byte little-endian length value. The length is actual file size, excluding alignment padding. Collisions with strong definitions are errors. No terminator is added; use the length.

For fixed inputs in the same order, output bytes are deterministic: timestamp zero, stable section order, sorted common allocation and a deterministic string table. Standard and BigObj inputs are accepted; output uses standard COFF and is limited to 32,767 sections. Extended relocation tables are emitted when necessary. Combined inputs/resources and final COFF output are each bounded to 64 MiB.

## Related interfaces

The TOML schema describes ordinary COFF composition. It does not interpret external linker scripts or apply specialized PIC/PICO source transformations. See [related projects](references.md) for ecosystem references.
