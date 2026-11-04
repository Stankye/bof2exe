# TrustedSec upstream validation

Validation uses the upstream prebuilt x64 objects and CNA from [trustedsec/CS-Situational-Awareness-BOF](https://github.com/trustedsec/CS-Situational-Awareness-BOF), pinned to commit [`fd96c4dcd88fd00eea902bebb20d0736e48f5f61`](https://github.com/trustedsec/CS-Situational-Awareness-BOF/tree/fd96c4dcd88fd00eea902bebb20d0736e48f5f61). Upstream source and binaries are kept outside the repository and release packages.

## Coverage

- All 72 published `.x64.o` objects pass static compatibility checks after the fixes below. Reports record a SHA-256 for every object.
- Eleven local, read-only commands exercise CNA analysis, schema generation, executable building, generated help/docs, and native standalone execution: `hostname`, `locale`, `resources`, `uptime`, `useridletime`, `whoami`, `winver`, `md5`, `sha256`, `cat`, and `dir`.
- File operations use a test-created file with spaces in its path. MD5 and SHA-256 results must match Windows `Get-FileHash`. Directory listing must contain that file, and `cat` must return its known contents.
- Each runtime case removes its copied BOF and generated schema before starting the generated executable. The process has a 30-second deadline. Native output is checked but is not included in the committed report.

Static compatibility means the object passes this loader's format, relocation, symbol, and entrypoint checks. It does not prove DLL exports are available on every system or that the remaining 61 modules execute correctly. Domain-, remote-host-, credential-, and mutation-dependent behavior was not exercised. x86 objects were excluded because execution currently supports x64.

## Fixes found by the corpus

The initial static pass accepted 64/72 objects. The remaining eight referenced `LoadLibraryA`, `GetModuleHandleA`, `GetProcAddress`, or `FreeLibrary` without a `KERNEL32$` prefix. These names now resolve through kernel32 for both direct calls and `__imp_` import pointers. Regression fixtures cover both import conventions.

CNA signature discovery previously required one `bof_pack` call, rejecting no-argument commands. It now accepts direct inline execution with an explicit `$null` buffer, or an omitted buffer, when every inline call has no buffer and no pack call exists. Omitted buffers produce a diagnostic: upstream `hostname` omits this argument. Helper-only commands and dynamic buffer expressions still require review. CNA scripts are never executed.

## Repeat the tests

From the project root on Windows x64 with Rust MSVC installed:

```powershell
git clone https://github.com/trustedsec/CS-Situational-Awareness-BOF.git target/trustedsec-upstream
git -C target/trustedsec-upstream checkout fd96c4dcd88fd00eea902bebb20d0736e48f5f61
./scripts/test-trustedsec.ps1 -Repository target/trustedsec-upstream
# Only inspect; do not execute any BOF:
./scripts/test-trustedsec.ps1 -Repository target/trustedsec-upstream -InspectOnly
```

The tests are opt-in and ignored during ordinary `cargo test`. They require no upstream build toolchain. JSON reports are written to `target/external-bofs/reports`, or the supplied `-ReportDirectory`.

For a separately downloaded/flattened object directory, set `BOF2EXE_TRUSTEDSEC_OBJECTS` and `BOF2EXE_TRUSTEDSEC_CNA` and run `cargo test --test external_trustedsec -- --ignored --nocapture`. The checked-in [validation snapshot](validation/trustedsec-2026-11-03.json) records this run's input hashes and results.
