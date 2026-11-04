mod cli;
mod generate;
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use bof2exe::{
    args::{pack, Argument},
    bundle::{self, Payload},
    inspect, link,
    manifest::Manifest,
    runtime,
};
use clap::{CommandFactory, Parser};
use cli::{Cli, Command, Input, ToolSource};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::{Command as Process, Stdio},
    time::{Duration, Instant},
};

fn main() {
    if let Err(error) = app() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
fn app() -> Result<()> {
    // Bundle dispatch is unconditional: wrapper arguments can never expose internal CLI commands.
    let executable = std::env::current_exe()?;
    if let Some(payload) = bundle::read(&mut File::open(&executable)?)? {
        // SAFETY: invocation opts into the program embedded by an explicit build/bundle.
        return unsafe { bof2exe::standalone::run_embedded(payload) };
    }
    match Cli::parse().command {
        Command::Init { source, output } => {
            ensure!(
                !is_tool_schema(&source.input),
                "init expects a BOF, not an existing tool schema"
            );
            let output = output.unwrap_or_else(|| source.input.with_extension("tool.toml"));
            let mut spec = tool_source(source)?;
            ensure_output_parent(&output)?;
            spec.object = relative_object(&spec.object, &output)?;
            let body=format!("# Parameters are packed in the order below. Review CNA-derived labels and help.\n{}",toml::to_string_pretty(&spec)?);
            new_file(&output)?.write_all(body.as_bytes())?;
            eprintln!(
                "Created {}. Build with: bof2exe build {} -o {}.exe",
                output.display(),
                output.display(),
                spec.name
            );
            Ok(())
        }
        Command::Build {
            source,
            output,
            docs,
        } => {
            ensure!(
                cfg!(all(windows, target_arch = "x86_64")),
                "standalone executable generation requires Windows x64"
            );
            let spec = tool_source(source)?;
            let definition = spec.definition();
            definition.validate()?;
            let object = bof2exe::read_input(&spec.object)?;
            compatible(&object, &spec.entry)?;
            let reference = docs.as_ref().map(|_| definition.markdown()).transpose()?;
            let mut payload = Payload::new(&object, spec.entry, &pack(&[])?);
            payload.tool = Some(definition);
            ensure!(
                serde_json::to_vec(&payload)?.len() <= bof2exe::MAX_INPUT,
                "encoded tool exceeds 64 MiB"
            );
            ensure!(
                !output.exists(),
                "output already exists: {}",
                output.display()
            );
            if let Some(path) = &docs {
                ensure!(
                    path != &output && !path.exists(),
                    "documentation output already exists or conflicts with executable"
                );
            }
            let runner = executable
                .parent()
                .context("converter has no parent directory")?
                .join(if cfg!(windows) {
                    "bof2exe-runner.exe"
                } else {
                    "bof2exe-runner"
                });
            let template = if runner.is_file() { runner } else { executable };
            ensure_output_parent(&output)?;
            if let Some(path) = &docs {
                ensure_output_parent(path)?;
            }
            let mut file = new_file(&output)?;
            bundle::write(File::open(template)?, &mut file, &payload)?;
            file.sync_all()?;
            if let (Some(path), Some(reference)) = (docs, reference) {
                new_file(&path)?.write_all(reference.as_bytes())?;
            }
            eprintln!("Created {}", output.display());
            Ok(())
        }
        Command::Inspect { input, json } => describe(input, json, false),
        Command::Check { input, json } => describe(input, json, true),
        Command::Run {
            input,
            timeout,
            json,
            arguments,
            parameters,
        } => {
            let (bytes, entry, defaults) = load(parameters.input_entry(input)?)?;
            let arguments = pack(&parameters.resolve(&arguments, &defaults)?)?;
            let output = worker(Payload::new(&bytes, entry, &arguments), timeout)?;
            print_output(output, json)
        }
        Command::Bundle {
            input,
            output,
            arguments,
            parameters,
        } => {
            ensure!(
                cfg!(all(windows, target_arch = "x86_64")),
                "bundling requires a Windows x64 runner"
            );
            let (bytes, entry, defaults) = load(parameters.input_entry(input)?)?;
            compatible(&bytes, &entry)?;
            let arguments = pack(&parameters.resolve(&arguments, &defaults)?)?;
            let payload = Payload::new(&bytes, entry, &arguments);
            // Validate serialized size before creating any output file.
            ensure!(
                serde_json::to_vec(&payload)?.len() <= bof2exe::MAX_INPUT,
                "bundle payload exceeds 64 MiB"
            );
            let mut file = new_file(&output)?;
            bundle::write(File::open(executable)?, &mut file, &payload)?;
            file.sync_all()?;
            eprintln!("Created {}", output.display());
            Ok(())
        }
        Command::Pack {
            output,
            arguments,
            parameters,
        } => {
            let data = pack(&parameters.resolve(&arguments, &[])?)?;
            if let Some(path) = output {
                new_file(&path)?.write_all(&data)?;
            } else {
                println!("{}", STANDARD.encode(data));
            }
            Ok(())
        }
        Command::Cna { path, json, docs } => {
            let report = bof2exe::cna::analyze_file(&path)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                generate::cna_docs(&report, docs)?;
            }
            Ok(())
        }
        Command::Link {
            objects,
            spec,
            output,
            plan: _,
            entry,
            json,
            libraries,
            whole_archive,
        } => {
            let (objects, resources, entry, libraries, whole_archive) = if let Some(path) = spec {
                let spec = link::Spec::load(&path)?;
                let mut all_libraries = spec.libraries;
                all_libraries.extend(libraries);
                (
                    spec.objects,
                    spec.resources,
                    entry.unwrap_or(spec.entry),
                    all_libraries,
                    whole_archive || spec.whole_archive,
                )
            } else {
                (
                    objects,
                    Vec::new(),
                    entry.unwrap_or_else(|| "go".into()),
                    libraries,
                    whole_archive,
                )
            };
            let names: Vec<_> = objects.iter().map(|p| p.display().to_string()).collect();
            ensure!(
                !objects.is_empty() && objects.len() <= 1024 && resources.len() <= 1024,
                "link input count exceeds limits"
            );
            let mut input_bytes = 0usize;
            let data = objects
                .iter()
                .map(|p| read_link_input(p, &mut input_bytes))
                .collect::<Result<Vec<_>>>()?;
            let resource_data = resources
                .iter()
                .map(|r| read_link_input(&r.path, &mut input_bytes))
                .collect::<Result<Vec<_>>>()?;
            ensure!(libraries.len() <= 1024, "too many libraries");
            let library_data = libraries
                .iter()
                .map(|p| read_link_input(p, &mut input_bytes))
                .collect::<Result<Vec<_>>>()?;
            let library_names: Vec<_> = libraries.iter().map(|p| p.display().to_string()).collect();
            let library_inputs: Vec<_> = library_names
                .iter()
                .zip(&library_data)
                .map(|(name, bytes)| link::Input { name, bytes })
                .collect();
            let inputs: Vec<_> = names
                .iter()
                .zip(&data)
                .map(|(name, bytes)| link::Input { name, bytes })
                .collect();
            let resource_inputs: Vec<_> = resources
                .iter()
                .zip(&resource_data)
                .map(|(r, bytes)| link::ResourceInput {
                    symbol: &r.symbol,
                    bytes,
                })
                .collect();
            let (linked, selection) =
                bof2exe::library::link(&inputs, &library_inputs, &resource_inputs, whole_archive)?;
            let compatibility = inspect::inspect(&linked.bytes, &entry)?;
            if let Some(path) = output {
                new_file(&path)?.write_all(&linked.bytes)?;
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"link":linked.report,"libraries":selection,"compatibility":compatibility})
                    )?
                );
            } else {
                println!(
                    "Linked {} objects into {} sections ({} bytes)",
                    linked.report.input_count,
                    linked.report.section_count,
                    linked.bytes.len()
                );
                for name in &linked.report.resolved {
                    println!("Resolved: {name}");
                }
                for member in &selection.selected {
                    println!("Library member: {member}");
                }
                for name in &linked.report.unresolved {
                    println!("External: {name}");
                }
                for issue in &compatibility.issues {
                    println!("Runtime issue: {issue}");
                }
            }
            Ok(())
        }
        Command::Library { path, json } => {
            let data = bof2exe::read_input(&path)?;
            let library = bof2exe::library::read(&data)?;
            let report = library.report()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}: {} members", report.format, report.members.len());
                for member in &report.members {
                    println!(
                        "{} ({}, {} bytes)\n  Defines: {}\n  References: {}",
                        member.name,
                        member.architecture,
                        member.size,
                        member.definitions.join(", "),
                        member.references.join(", ")
                    );
                }
                for ignored in &report.ignored {
                    println!("Metadata: {ignored}");
                }
            }
            Ok(())
        }
        Command::Suite {
            path,
            execute,
            json,
        } => {
            ensure!(
                !execute || cfg!(all(windows, target_arch = "x86_64")),
                "suite execution requires Windows x64"
            );
            let suite = bof2exe::suite::Suite::load(&path)?;
            let report =
                bof2exe::suite::run(&suite, execute, |object, entry, arguments, timeout| {
                    worker(Payload::new(object, entry.into(), arguments), timeout)
                });
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "{} [{}]: {} passed, {} failed",
                    report.name, report.mode, report.passed, report.failed
                );
                for case in &report.cases {
                    println!(
                        "{} {}",
                        if case.passed { "PASS" } else { "FAIL" },
                        case.name
                    );
                    for diagnostic in &case.diagnostics {
                        println!("  {diagnostic}");
                    }
                }
            }
            ensure!(report.failed == 0, "fixture suite failed");
            Ok(())
        }
        Command::Capabilities { json: _ } => {
            let capabilities = serde_json::json!({
                "execution": "windows-x86_64", "object_formats": ["coff", "bigobj"],
                "library_formats": ["coff-archive (.lib/.a)", "ZIP32 stored/deflate object collections"],
                "relocations": ["ABSOLUTE", "ADDR64", "ADDR32", "ADDR32NB", "REL32", "REL32_1", "REL32_2", "REL32_3", "REL32_4", "REL32_5", "SECTION", "SECREL"],
                "beacon_apis": inspect::BEACON_APIS, "crt_helpers": inspect::CRT_APIS, "compiler_helpers": inspect::COMPILER_APIS,
                "loader_apis": inspect::LOADER_APIS,
                "linking": ["multiple objects", "lazy or whole library members", "strong/common symbols", "weak aliases (NOLIBRARY/LIBRARY/ALIAS)", "COMDAT ANY/SAME_SIZE/EXACT_MATCH/LARGEST/NEWEST and associative groups", "embedded resources", "extended relocations"],
                "fixture_suites": ["static inspection", "expected incompatibility", "worker execution", "callback byte expectations", "JSON reports"],
                "unwind": "validated AMD64 version-1 function table registration for synchronous calls",
                "cna": "static metadata; literal bof_pack signatures; no script evaluation",
                "remaining": ["x86/ARM64 execution", "import libraries/LTCG", "thin archives/ZIP64/encrypted ZIP", "COMDAT EXACT_MATCH weak or cross-section local targets", "TLS/static constructors", "general linker directives", "full C++ runtime", "session-dependent Beacon APIs", "asynchronous object lifetimes", "PICO/PIC conventions"]
            });
            println!("{}", serde_json::to_string_pretty(&capabilities)?);
            Ok(())
        }
        Command::Docs { output } => generate::docs(Cli::command(), &output),
        Command::Completions { shell } => generate::completions(Cli::command(), shell),
        Command::Module { manifest, docs } => {
            let module = Manifest::load(&manifest)?;
            if docs {
                generate::module_docs(&module)?;
            } else {
                println!("{}", toml::to_string_pretty(&module)?);
            }
            Ok(())
        }
        Command::Worker => {
            let mut data = Vec::new();
            std::io::stdin()
                .take(bof2exe::MAX_INPUT as u64 * 3 + 1)
                .read_to_end(&mut data)?;
            ensure!(
                data.len() <= bof2exe::MAX_INPUT * 3,
                "worker request too large"
            );
            let payload: Payload = serde_json::from_slice(&data)?;
            let (object, mut arguments) = payload.decode()?;
            // SAFETY: the operator explicitly requested execution of this local native object.
            let output = unsafe { runtime::execute(&object, &payload.entry, &mut arguments)? };
            serde_json::to_writer(std::io::stdout().lock(), &output)?;
            Ok(())
        }
    }
}

fn new_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create {} (output must not already exist)", path.display()))
}
fn read_link_input(path: &Path, total: &mut usize) -> Result<Vec<u8>> {
    let data = bof2exe::read_input(path).with_context(|| format!("read {}", path.display()))?;
    *total = total
        .checked_add(data.len())
        .context("link input size overflow")?;
    ensure!(
        *total <= bof2exe::MAX_INPUT,
        "combined link inputs exceed 64 MiB"
    );
    Ok(data)
}
fn load(input: Input) -> Result<(Vec<u8>, String, Vec<Argument>)> {
    let (path, entry, defaults) = if let Some(path) = input.manifest {
        let module = Manifest::load(&path)?;
        (
            module.object,
            input.entry.unwrap_or(module.entry),
            module.arguments,
        )
    } else {
        (
            input.object.context("missing object")?,
            input.entry.unwrap_or_else(|| "go".into()),
            Vec::new(),
        )
    };
    let bytes = bof2exe::read_input(&path).with_context(|| format!("read {}", path.display()))?;
    Ok((bytes, entry, defaults))
}
fn compatible(bytes: &[u8], entry: &str) -> Result<()> {
    let report = inspect::inspect(bytes, entry)?;
    ensure!(
        report.compatible(),
        "incompatible object:\n{}",
        report.issues.join("\n")
    );
    Ok(())
}
fn describe(input: Input, json: bool, strict: bool) -> Result<()> {
    let (bytes, entry, _) = load(input)?;
    let report = inspect::inspect(&bytes, &entry)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Architecture: {}\nSHA-256: {}\nEntrypoint: {entry}\nStatic compatibility: {}",
            report.architecture,
            report.sha256,
            if report.compatible() { "pass" } else { "fail" }
        );
        for section in &report.sections {
            println!(
                "  {:<20} {:>8} bytes  {:>5} relocations",
                section.name, section.size, section.relocations
            );
        }
        if !report.imports.is_empty() {
            println!("Imports:\n  {}", report.imports.join("\n  "));
        }
        for note in &report.notes {
            println!("Note: {note}");
        }
        for issue in &report.issues {
            println!("Issue: {issue}");
        }
    }
    ensure!(
        !strict || report.compatible(),
        "static compatibility checks failed"
    );
    Ok(())
}

fn worker(payload: Payload, timeout: u64) -> Result<Vec<runtime::Output>> {
    ensure!(
        cfg!(all(windows, target_arch = "x86_64")),
        "execution requires Windows x64"
    );
    let (object, _) = payload.decode()?;
    compatible(&object, &payload.entry)?;
    let request = serde_json::to_vec(&payload)?;
    let mut child = Process::new(std::env::current_exe()?)
        .arg("__worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut stdout = child.stdout.take().context("worker stdout missing")?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            (&mut stdout)
                .take(bof2exe::MAX_INPUT as u64 * 5 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= bof2exe::MAX_INPUT * 5,
                "worker output too large"
            );
            Ok(bytes)
        })();
        let _ = sender.send(result);
    });
    let sent = child
        .stdin
        .take()
        .context("worker stdin missing")?
        .write_all(&request);
    if let Err(error) = sent {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            // Do not join a pipe reader: a native BOF could have spawned a process inheriting stdout.
            anyhow::bail!("worker exceeded {timeout}s deadline and was terminated");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    ensure!(status.success(), "worker failed with {status}");
    let data = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .context("worker output did not close before the deadline")??;
    serde_json::from_slice(&data)
        .context("invalid worker output (the object may have written directly to stdout)")
}
fn print_output(output: Vec<runtime::Output>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for item in output {
            if item.kind == 13 {
                std::io::stderr().write_all(&item.data)?;
            } else {
                std::io::stdout().write_all(&item.data)?;
            }
        }
    }
    Ok(())
}
fn is_tool_schema(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
}
fn tool_source(source: ToolSource) -> Result<bof2exe::tool::Spec> {
    let mut spec = if is_tool_schema(&source.input) {
        ensure!(source.parameters.cna.is_none()&&source.parameters.format.is_none(),"CNA/format inputs apply to direct BOF conversion; edit the schema's parameters instead");
        bof2exe::tool::Spec::load(&source.input)?
    } else {
        let bytes = bof2exe::read_input(&source.input)?;
        bof2exe::coff::Coff::parse(&bytes)?;
        let stem = source
            .input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("tool");
        let stem = stem
            .strip_suffix(".x64")
            .or_else(|| stem.strip_suffix(".x86"))
            .unwrap_or(stem);
        let name = source.name.clone().unwrap_or_else(|| {
            let name = bof2exe::tool::sanitize(stem);
            if name.is_empty() {
                "tool".into()
            } else {
                name
            }
        });
        let (format, description, usage, entry) = if let Some(path) = &source.parameters.cna {
            let report = bof2exe::cna::analyze_file(path)?;
            let command = source.parameters.command.as_deref().unwrap_or_default();
            let format = report
                .signature(command, source.parameters.variant)?
                .to_string();
            let metadata = report
                .commands
                .iter()
                .find(|c| c.name == command)
                .context("CNA command missing")?;
            ensure!(
                source.entry.is_some() || metadata.entrypoints.len() <= 1,
                "CNA has multiple entrypoints; select --entry"
            );
            (
                format,
                metadata.description.clone().unwrap_or_default(),
                metadata.usage.clone(),
                metadata.entrypoints.first().cloned(),
            )
        } else {
            (
                source.parameters.format.clone().unwrap_or_default(),
                String::new(),
                None,
                None,
            )
        };
        let mut spec = bof2exe::tool::scaffold(
            source.input.clone(),
            name,
            &format,
            description,
            usage.as_deref(),
        )?;
        if let Some(entry) = entry {
            spec.entry = entry;
        }
        spec
    };
    if let Some(name) = source.name {
        spec.name = name;
    }
    if let Some(description) = source.description {
        spec.description = description;
    }
    if let Some(entry) = source.entry {
        spec.entry = entry;
    }
    ensure!(
        !spec.entry.is_empty() && !spec.entry.contains('\0'),
        "invalid entrypoint"
    );
    spec.definition().validate()?;
    Ok(spec)
}
fn relative_object(object: &Path, output: &Path) -> Result<std::path::PathBuf> {
    let object = std::fs::canonicalize(object)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = std::fs::canonicalize(parent)?;
    let a: Vec<_> = object.components().collect();
    let b: Vec<_> = parent.components().collect();
    let common = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return Ok(object);
    }
    let mut relative = std::path::PathBuf::new();
    for _ in common..b.len() {
        relative.push("..");
    }
    for component in &a[common..] {
        relative.push(component.as_os_str());
    }
    Ok(relative)
}
fn ensure_output_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    Ok(())
}
