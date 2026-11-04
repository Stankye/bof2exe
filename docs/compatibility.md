# Compatibility matrix

Version 0.4.1 converts BOFs into standalone Windows x64 CLIs. This matrix describes the supported input formats and runtime behavior. `bof2exe capabilities --json` provides a machine-readable inventory; `check OBJECT --json` evaluates an individual object. Static compatibility checks do not guarantee successful native execution.

| Area | Implemented | Remaining limits |
| --- | --- | --- |
| File formats | Standard COFF and BigObj, checked names/auxiliary records, BSS, extended relocations | No import-library members, LTCG, or PE input |
| Libraries | Self-contained .lib/.a archives; stored/deflated ZIP32 object collections; metadata inventory and lazy/whole selection | No thin archives, ZIP64, encryption, implicit dependency acquisition, or non-COFF members |
| Architectures | x64 execution; x86/ARM64 parsing | x86/ARM64 execution requires additional backends |
| Relocations | AMD64 ABSOLUTE, ADDR64/ADDR32/ADDR32NB, REL32 through REL32_5, SECTION, SECREL | Out-of-range values fail; no implicit low-address allocation for ADDR32 |
| Symbols | Strong/static symbols, largest-common allocation, strong-over-common, weak NOLIBRARY/LIBRARY/ALIAS and cycle detection | Anti-dependencies rejected; library search uses explicitly supplied inputs |
| Composition | Multiple objects, libraries/resources, deterministic standard COFF output | Ordinary duplicate strong definitions rejected; no general identical-code folding |
| COMDAT | ANY, SAME_SIZE, EXACT_MATCH, LARGEST, NEWEST, NODUPLICATES and associative chains | Conservative errors for EXACT_MATCH weak/cross-section local targets or unmappable live locals in discarded sections |
| Imports | Supported Beacon names, CRT helpers, stack probes, four unqualified loader APIs, DLL$Export and __imp_ convention | DLL exports must exist in System32; unknown references fail |
| Beacon data | Parse/Ptr/Int/Short/Length/Extract | Pointer validity remains the native caller's responsibility |
| Beacon output | Output/Printf, original callback kinds and raw bytes; BeaconDownload captures named local artifacts | Synchronous executing-thread callbacks; 64 MiB shared capture limit; no C2 download protocol. See [download behavior](downloads.md). |
| Beacon format | Alloc/Reset/Free/Append/Printf/ToString/Int | No full Beacon session state |
| Utility API | toWideChar with Windows ANSI conversion | No token, process-injection, data-store, syscall, or gate session emulation |
| CRT helpers | memcpy/memmove/memset/memcmp, strlen/strcmp/strncmp/strcpy/strncpy/strcat/strncat, case-insensitive and wide comparisons listed by capabilities | Not a complete statically linked CRT; no constructors/TLS initialization |
| Stack probes | MSVC __chkstk and GNU ___chkstk_ms/__chkstk_ms | Windows x64 ABI only |
| Unwind metadata | Sorted, checked AMD64 version-1 function tables; registration scoped to execution | No C++ runtime initialization; native faults still terminate worker |
| CNA | Registrations, alias/sub bodies, literal/constant formats, explicit null/omitted buffers, wire expressions, paths/entrypoint hints, variant selection | Conservative subset; no interpreter, includes, helper evaluation or remote code-page inference |
| Fixture suites | Static expectations, original hashes, expected incompatibility, worker execution, callback byte assertions, JSON results | Output assertions are deferred in inspection mode; native paths require Windows x64 |
| Generated CLIs | Command-specific named/short/positional arguments, defaults, choices, numeric switches, binary files, help/version/docs; object and schema embedded | Windows x64; input BOF must meet the runtime contract; native faults can terminate the tool |
| Templates/bundles | Smaller runner template for `build`; legacy raw-argument `bundle` remains supported | Checksum is integrity metadata, not a signature |

## Verification

Original fixtures exercise real native execution of multiple linked objects/resources, MSVC C++ BigObj and shared inline COMDATs, actual MSVC .lib creation, deflated ZIP libraries, callback formatting, large stack frames, CRT initialization helpers, unwind lookup, standalone packaging, timeouts and native faults. Portable tests cover container decoding, truncation/CRC/range failures, lazy transitive selection, COMDAT semantics, signatures, symbols, suite expectations, deterministic output and bundle integrity.

CI is configured for Linux and Windows. Local native verification uses Windows x64 MSVC Rust and automatically discovered MSVC C++ Build Tools. The [TrustedSec corpus validation](trustedsec-validation.md) covers 72 upstream x64 objects statically and eleven generated standalone CLIs at runtime, using pinned unmodified artifacts. Foreign CNA sources, Garden implementations, and BOF collections are not vendored into the permissively licensed test suite.
