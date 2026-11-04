# COFF libraries

```powershell
bof2exe library helpers.lib --json
bof2exe library helpers.zip --json
bof2exe link main.o -L helpers.lib -L utilities.zip -o linked.o --json
```

The inventory reports member ordinal/name, architecture, size, definitions and referenced externals. Linking includes root objects, then selects members needed by live relocations. It rechecks earlier libraries after selecting a member, so transitive dependencies across supplied libraries do not require manual repetition. Ties follow supplied library/member order. Reports list selected and unused members by stable labels. Same-named archive members retain distinct ordinals.

`--whole-archive` explicitly includes every member. Mixed architectures in an inventory are allowed; lazy selection chooses the root architecture, while whole-library inclusion of another architecture fails. Ordinary duplicate strong definitions remain errors.

```toml
schema = 1
name = "library-example"
entry = "go"
objects = ["main.o"]
libraries = ["helpers.lib", "utilities.zip"]
whole_archive = false
```

Paths in this spec resolve beside the spec; command-line `--library` paths resolve from the current directory. Command-line libraries append to spec libraries. No library name causes implicit filesystem or network searches.

## Containers

The archive reader accepts self-contained Microsoft/GNU/BSD ar-style archives: short names, Windows/GNU longname tables, BSD inline long names and member alignment. Index members are recognized as metadata; selection uses parsed symbols rather than trusting the index. Thin archives are rejected because their contract acquires external files. Short import-library and LTCG members are outside this COFF object reader.

ZIP32 collections may store or deflate `.o`/`.obj` entries. The reader checks central/local metadata, lengths, overlapping ranges, decoded limits and object CRC-32. Non-object entries are recorded as ignored metadata. It does not extract members to disk, execute archive content, follow symlinks, or reinterpret object hints as file paths. Encryption, ZIP64, multiple disks and other compression methods are rejected.

Root/library file input is bounded to 64 MiB. Combined decoded object contents, selected input and emitted COFF also have bounds. Libraries are limited to 4,096 aggregate object members, with at most 1,024 selected root/member objects accepted by the partial linker. Inputs too large for standard output fail before execution.

## COMDAT and weak definitions

COMDAT sections are selected before ordinary symbol resolution and before calculating live library references. ANY keeps the first; SAME_SIZE checks size; EXACT_MATCH checks bytes and relocation meaning; LARGEST chooses size; NEWEST chooses the COFF input timestamp, with first-input ties. NODUPLICATES rejects duplicates. Associative chains follow the chosen parent, with invalid parents and cycles rejected.

Relocations to discarded external definitions redirect to the selected definition. Section symbols can map to a uniquely corresponding kept section. A live reference to an unmappable discarded local is an error. EXACT_MATCH conservatively rejects weak and cross-section local targets that cannot be proven equivalent. Conflicting selection modes are errors. This implements declared COMDAT selection, not general identical-code folding.

Weak LIBRARY references search explicitly supplied libraries before following their fallback. NOLIBRARY/ALIAS references follow their fallback without searching for the weak name. No implicit libraries are acquired, and anti-dependency mode remains unsupported. Common declarations retain largest-size allocation and strong-over-common resolution.

Container and object semantics follow [Microsoft's PE/COFF specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format), [LLVM's archive documentation](https://llvm.org/docs/CommandGuide/llvm-ar.html), and the [ZIP format reference](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).
