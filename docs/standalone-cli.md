# BOF to standalone command-line tool

`bof2exe build` embeds a local BOF and its command schema into a Windows executable. The generated program has its own flags, help, version and validation. It needs neither the converter, source BOF, schema, Rust, nor a C compiler at runtime.

## Start with CNA metadata

```powershell
bof2exe init hello.o --cna hello.cna --command hello -o hello.tool.toml
# Review names/help/defaults in hello.tool.toml.
bof2exe build hello.tool.toml -o hello.exe --docs hello.md
./hello.exe --help
./hello.exe --name Alice --count 3
```

For a literal, unambiguous CNA signature, direct conversion is also supported:

```powershell
bof2exe build hello.o --cna hello.cna --command hello -o hello.exe
```

CNA usage placeholders such as `<count> <name>` draft parameter names when their count matches the wire signature. Otherwise the draft uses `arg1`, `arg2`, etc. The generated schema is the place to review semantic labels, defaults and help. Script expressions/defaults/validation are not executed or guessed. Without CNA, provide `--format iz` to draft integer/string parameters; without a signature, the tool has no declared arguments.

Direct CNA commands that pass `$null`, or omit the inline-execution argument buffer entirely, can draft a no-argument tool without `bof_pack`. An omitted buffer produces a diagnostic in the CNA report. Commands forwarding dynamic buffers or using helpers still require review.

## Schema

```toml
schema = 1
name = "hello"
version = "1.0.0"
description = "Print a greeting"
object = "hello.o"
entry = "go"

[[parameters]]
name = "count"
type = "int"
short = "c"
help = "Number to include in the greeting"
default = "7"

[[parameters]]
name = "name"
type = "str"
short = "n"
help = "Person to greet"
```

`parameters` are packed in **BOF wire order**, regardless of the order in which flags are supplied. The object path resolves beside the schema and is embedded at build time. `--name`, `--description`, and `--entry` can override schema values when building. Existing output files are never overwritten.

| Setting | Behavior |
| --- | --- |
| `type` | `str`, `wstr`, signed `int`/`short`, or `bin` |
| `short` | Optional single-letter alias, such as `-n` |
| `help` | Description shown in the generated program's help |
| `default` | String representation of a default value; numeric defaults are validated at build time |
| `required` | Defaults to true; a declared default makes the option optional |
| `positional = true` | Accept a positional value instead of a named option |
| `switch = true` | For int/short fields, bare `--flag` packs 1; `--flag=false` packs 0 |
| `choices = [...]` | Restrict allowed string representations |
| `encoding = "file"` | For `bin`, read a runtime input file |
| `encoding = "hex"` | For `bin`, decode raw hex; binary defaults to raw base64 |

Optional fields without defaults still occupy their wire position: empty strings/blobs or zero integers are packed. Switch defaults use `"true"`/`"false"`, and can be overridden with `--flag=false` or `--flag=true`. Required positional fields precede optional positionals. `help`, `version`, `-h` and `-V` are reserved. Invalid schemas are rejected before an executable is created.

For a file argument, for example:

```toml
[[parameters]]
name = "input"
type = "bin"
encoding = "file"
help = "Input file"
```

The generated program accepts `--input example.bin`; it reads that data when invoked, and embeds no dependency on the original BOF path. Help/version requests do not execute the object or read input data files.

## Output and exit codes

`BeaconDownload` output is saved as [local artifacts](downloads.md) in a fresh directory beneath the working directory, with paths reported on stdout. Each named buffer is preserved without overwriting another download. Artifact write failures make the CLI fail.

Callback output is written as raw bytes; error callback kind 13 goes to stderr. Generated tools return 2 for argument/validation errors and 1 for reported module errors. Set `error_exit = false` at the schema root if an object's error callbacks should not change its exit status. Help/version return 0. Native execution occurs in the generated program's own process; an object fault can terminate it. The runtime's existing Windows x64 compatibility limits still apply.

The converter prefers the sibling `bof2exe-runner.exe` template to keep output smaller. Keep the converter and template from the same release together when building. If the template is absent, the converter can use its own executable image; the generated command still exposes only its declared CLI. The runner and object are copied into the final executable, so distributing the generated tool requires only that executable.

`bundle` provides a raw `TYPE:VALUE` runner interface. Use `build` for command-line tools with named flags. Inspection, library linking, and suites help prepare or diagnose an input BOF.
