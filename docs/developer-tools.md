# Developer tools

The main workflow is [`init` and `build`](standalone-cli.md). These commands help inspect an input, diagnose compatibility, prepare linked objects, or embed the library in another application. Examples assume `bof2exe` is on `PATH`.

## Inspect and run an object

```powershell
bof2exe inspect hello.o --json
bof2exe check hello.o --json
bof2exe run hello.o --timeout 10 -- int:7 str:world
bof2exe run --manifest examples/hello.toml --json
```

`inspect` reports compatibility issues without failing solely because an object is unsupported. `check` returns a nonzero status for incompatibility. Both reject malformed input. Static checks do not guarantee that DLL exports are available or that native execution will succeed.

`run` executes in a child process with a deadline. JSON output preserves callback kinds and byte arrays. Text output sends callback kind 13 to stderr and other callbacks to stdout. Direct writes from a BOF to the worker's stdout interfere with its JSON protocol.

Module manifests such as [examples/hello.toml](../examples/hello.toml) define an object, entrypoint, and ordered typed defaults. Their paths resolve beside the manifest. Explicit arguments after `--` replace the full default list; `--entry` overrides the entrypoint. A module manifest is distinct from a [tool schema](standalone-cli.md), which defines a generated CLI's flags.

## Pack arguments

```powershell
bof2exe pack -- int:42 short:-1 str:hello wstr:world hex:00ff bin:AQID
bof2exe pack -o arguments.bin -- int:42 str:hello
bof2exe pack --cna examples/hello.cna --command hello -- 7 world
```

Without `-o`, `pack` prints base64. With `-o`, it writes raw bytes to a new file. The buffer begins with its little-endian payload length. Integers are little-endian; strings and blobs carry length prefixes. `str` uses UTF-8, `wstr` uses UTF-16LE, and both include a terminator. Strings reject interior NULs. CNA-assisted values are final values in wire order; script expressions are not executed.

## Build a C object

From an x64 Visual Studio developer terminal:

```powershell
cl /nologo /c /O2 /GS- /Zl /Iinclude examples/hello.c /Fohello.o
bof2exe init hello.o --format iz -o hello.tool.toml
bof2exe build hello.tool.toml -o hello.exe
```

The supplied [Beacon header](../include/beacon.h) declares the supported API subset. `/Zl` suppresses default CRT linker directives. See the [compatibility matrix](compatibility.md) for compiler and runtime limits.

## Raw bundles

`bundle` keeps the older typed-argument runner interface. Use `build` for tools with named flags.

```powershell
bof2exe bundle hello.o -o runner.exe -- int:7 str:world
./runner.exe
./runner.exe -- int:3 str:Alice
```

No arguments uses the embedded defaults. `--` followed by typed values replaces them; a bare `--` clears them. Raw bundles embed the object and entrypoint and execute in their own process.

## Rust API

```rust
use bof2exe::{args::{pack, Argument}, inspect::inspect, runtime};

fn run() -> anyhow::Result<()> {
    let bytes = std::fs::read("hello.o")?;
    let report = inspect(&bytes, "go")?;
    anyhow::ensure!(report.compatible(), "{:?}", report.issues);
    let mut args = pack(&[Argument::Int(7), Argument::Str("world".into())])?;
    // SAFETY: the caller has reviewed the object's code, ABI, and synchronous lifetime.
    let output = unsafe { runtime::execute(&bytes, "go", &mut args)? };
    for callback in output {
        println!("kind={} bytes={}", callback.kind, callback.data.len());
    }
    Ok(())
}
```

The execution API is unsafe and runs in the caller's process. The object must return synchronously and leave no threads or callbacks referring to its mapping. `cargo doc --no-deps` documents the public API and its contract.

## Reference generation

```powershell
bof2exe docs --output docs/generated
bof2exe completions bash
bof2exe completions power-shell
bof2exe module examples/hello.toml --docs
```

CLI Markdown, roff manuals, and completion candidates use the same command model as `--help`. Completion suggests command and option names; typed values are not completed. Module documentation combines a manifest with object inspection. Generated CLI documentation uses `build ... --docs tool.md`.
