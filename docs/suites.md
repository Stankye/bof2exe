# Local fixture suites

Suites record an object, arguments, and expected behavior for repeatable compatibility checks.

```powershell
./scripts/prepare-fixtures.ps1
bof2exe suite examples/suite.toml --json
bof2exe suite examples/suite.toml --execute --json
```

The preparation script uses the Windows x64 MSVC Rust toolchain to compile fixtures into `target/fixtures`. Inspection is portable and does not execute objects. Execution requires Windows x64 and uses the same worker/deadline path as `run`. Expected output is checked only with `--execute`; inspection records those assertions as deferred. Release packages include the prepared inputs, so they do not need this compilation step.

```toml
schema = 1
name = "greeting regression"

[[cases]]
name = "typed greeting"
object = "hello.o"
entry = "go"
timeout = 10
arguments = [{ type = "int", value = 7 }, { type = "str", value = "world" }]

[[cases.expect]]
kind = 0
contains = "hello world: count=7"

[[cases]]
name = "missing entrypoint"
object = "hello.o"
entry = "absent"
compatible = false
issue_contains = "entrypoint"
```

Object paths resolve beside the suite file. Case names must be unique. Schema/unknown fields, entrypoints, deadlines, output expectation syntax and case counts are validated before processing. A compatible case is the default; an incompatible case checks a parsed object's compatibility issues and is never executed. Unreadable or malformed inputs count as failures rather than satisfying an expected incompatibility.

Output clauses concatenate callback bytes in their original order, optionally filtered by `kind`. `contains` matches UTF-8 bytes; `exact_base64` compares arbitrary bytes, including NULs, exactly. If both are present, both must pass. A case accepts up to 64 clauses. A native failure, timeout, compatibility mismatch or failed output assertion records a failure and processing continues to later cases.

JSON results include the suite name/mode, passed/failed totals, each case's name, original object SHA-256, whether execution was attempted, status and diagnostics. The CLI returns nonzero when any case fails, after emitting the report. Inspection success only certifies the static expectation, and execution success only covers the code paths and assertions in that fixture. Suites do not create a security sandbox.
