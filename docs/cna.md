# CNA metadata and arguments

`bof2exe cna` reads a UTF-8 CNA file and produces static metadata. It never starts a Sleep VM, executes Aggressor callbacks, follows includes, acquires files named by the script, or connects to a Beacon.

```powershell
bof2exe cna examples/hello.cna --json
bof2exe cna examples/hello.cna --docs
bof2exe pack --cna examples/hello.cna --command hello -- 7 world
bof2exe run hello.o --cna examples/hello.cna --command hello -- 7 world
```

## What is extracted

- Literal `beacon_command_register` name/description/usage metadata.
- Named `alias` and `sub` bodies, with source line numbers.
- `bof_pack` calls and parameter expressions in their original wire order.
- A literal format, or a format variable assigned exactly once to an unconditional literal before the call. Compound mutations invalidate inference.
- Literal entrypoint names in `beacon_inline_execute`.
- No-argument signatures when every direct inline execution passes `$null` or omits its buffer and the command has no packing calls. Omitted buffers produce a diagnostic; helper-only and dynamic-buffer commands are not inferred as empty.
- `script_resource` / `readbof` argument expressions as object hints, and helper calls for review.

The lexer handles single/double quoted strings, escapes, line/block comments, nested delimiters, and commas inside nested calls or literals. Double-quoted interpolation is dynamic. It limits source size, token count, and nesting; malformed syntax is an error. This is a conservative analysis subset, not a complete grammar for Sleep regexes, heredocs, metaprogramming, or arbitrary script behavior.

## Type mapping

The [documented `bof_pack` formats](https://hstechdocs.helpsystems.com/manuals/cobaltstrike/current/userguide/content/topics_aggressor-scripts/as-resources_functions.htm#bof_pack) correspond to these local types:

| Format | Local type | Input after `--` |
| --- | --- | --- |
| `i` | signed 32-bit integer | `42`, `-1` |
| `s` | signed 16-bit integer | `12`, `-2` |
| `z` | NUL-terminated byte string | raw text, locally encoded as UTF-8 |
| `Z` | NUL-terminated wide string | raw text, UTF-16LE |
| `b` | length-prefixed binary | base64, `bin:BASE64`, or `hex:HEX` |

`z` does not infer a remote machine's ANSI code page. Use an explicitly encoded binary value and a reviewed layout when byte-for-byte locale behavior is needed. `--format` uses the same mapping without a CNA file. Without either option, the original `TYPE:VALUE` interface remains available.

## Ambiguity and transformations

For `bof_pack($1, "iz", int($2), transform($3))`, discovery reports two wire fields and preserves `int($2)` and `transform($3)` as expressions. The caller supplies the final integer and string. Defaults, expressions, transforms, validation and conditionals are not evaluated. Input count/range errors are rejected before execution.

Multiple `bof_pack` calls require `--variant N`, indexed in source order from one. A dynamic format cannot be selected for automatic packing. Review the report and supply an explicit `--format` instead. A wrapper calling a packing helper reports the helper name; select the helper's signature explicitly after reviewing its argument mapping. Automatically reusing a helper signature for the wrapper could reorder or invent parameters.

For execution, the object path remains explicit. A unique literal entrypoint is inferred from the selected CNA command when neither a manifest nor `--entry` provides one. Multiple entrypoint names require `--entry`. Object hints never trigger automatic acquisition or architecture guessing.

## Output

JSON preserves command kind, source lines, descriptions, usages, formats, parameter expressions, object hints, entrypoints, helper names and diagnostics. Markdown output turns this metadata into a command reference. Script contents are input data; their license is independent of this project's parser. All repository CNA fixtures were authored for this project.
