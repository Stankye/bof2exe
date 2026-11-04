//! Opt-in tests against unmodified, separately obtained upstream objects.
//! No third-party source or binaries are included in this test module.
use std::{
    fs,
    path::{Path, PathBuf},
};

fn objects() -> PathBuf {
    PathBuf::from(
        std::env::var_os("BOF2EXE_TRUSTEDSEC_OBJECTS").expect(
            "set BOF2EXE_TRUSTEDSEC_OBJECTS to a directory containing upstream x64 objects",
        ),
    )
}
fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            collect(&entry.path(), files);
        } else if entry.file_name().to_string_lossy().ends_with(".x64.o") {
            files.push(entry.path());
        }
    }
}
fn write_report(name: &str, value: &impl serde::Serialize) {
    let directory = std::env::var_os("BOF2EXE_TRUSTEDSEC_REPORTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/external-bofs/reports"));
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join(name),
        serde_json::to_vec_pretty(value).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires an external TrustedSec corpus; see docs/trustedsec-validation.md"]
fn all_upstream_x64_objects_pass_static_compatibility() {
    let root = objects();
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no .x64.o objects found");
    let reports: Vec<_> = files
        .iter()
        .map(|path| {
            let report = bof2exe::inspect::inspect(&fs::read(path).unwrap(), "go").unwrap();
            serde_json::json!({"object":path.strip_prefix(&root).unwrap(), "report":report})
        })
        .collect();
    write_report("trustedsec-static.json", &reports);
    let failures: Vec<_> = reports
        .iter()
        .filter(|r| !r["report"]["issues"].as_array().unwrap().is_empty())
        .collect();
    assert!(
        failures.is_empty(),
        "incompatible upstream objects: {failures:#?}"
    );
    println!(
        "{} upstream x64 objects passed static inspection",
        reports.len()
    );
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::*;
    use std::{
        io::Read,
        process::{Command, Output, Stdio},
        thread,
        time::{Duration, Instant},
    };

    fn run(command: &mut Command) -> Output {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let out = thread::spawn(move || {
            let mut b = Vec::new();
            stdout.read_to_end(&mut b).unwrap();
            b
        });
        let err = thread::spawn(move || {
            let mut b = Vec::new();
            stderr.read_to_end(&mut b).unwrap();
            b
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("command exceeded 30s: {command:?}");
            }
            thread::sleep(Duration::from_millis(10));
        };
        Output {
            status,
            stdout: out.join().unwrap(),
            stderr: err.join().unwrap(),
        }
    }
    fn converter() -> Command {
        Command::new(env!("CARGO_BIN_EXE_bof2exe"))
    }
    fn hash(path: &Path, algorithm: &str) -> String {
        // Windows' independent digest implementation provides the oracle for the upstream BOF.
        let output = run(Command::new("powershell")
            .args(["-NoProfile", "-Command"])
            .arg(format!(
                "(Get-FileHash -LiteralPath '{}' -Algorithm {algorithm}).Hash",
                path.display().to_string().replace('\'', "''")
            )));
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }
    #[test]
    #[ignore = "executes eleven reviewed local/read-only upstream BOFs; see docs/trustedsec-validation.md"]
    fn cna_to_standalone_tools_execute_with_real_arguments() {
        let mut files = Vec::new();
        collect(&objects(), &mut files);
        let cna = PathBuf::from(
            std::env::var_os("BOF2EXE_TRUSTEDSEC_CNA")
                .expect("set BOF2EXE_TRUSTEDSEC_CNA to upstream SA.cna"),
        );
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input with spaces.txt");
        fs::write(&input, b"bof2exe-upstream-fixture-2026\n").unwrap();
        let md5 = hash(&input, "MD5");
        let sha256 = hash(&input, "SHA256");
        let mut reports = Vec::new();
        for (name, expected) in [
            ("hostname", "Computer Name:"),
            ("locale", "LCID:"),
            ("resources", "Memory Used:"),
            ("uptime", "Uptime:"),
            ("useridletime", "Current User idle time:"),
            ("whoami", "GROUP INFORMATION"),
            ("winver", "Build Number:"),
            ("md5", md5.as_str()),
            ("sha256", sha256.as_str()),
            ("cat", "bof2exe-upstream-fixture-2026"),
            ("dir", "input with spaces.txt"),
        ] {
            let object = files
                .iter()
                .find(|p| p.file_name().unwrap() == format!("{name}.x64.o").as_str())
                .expect("upstream object missing");
            let schema = directory.path().join(format!("{name}.tool.toml"));
            let copied_object = directory.path().join(format!("{name}.x64.o"));
            fs::copy(object, &copied_object).unwrap();
            let executable = directory.path().join(format!("{name}.exe"));
            let docs = directory.path().join(format!("{name}.md"));
            let initialized = run(converter()
                .arg("init")
                .arg(&copied_object)
                .arg("--cna")
                .arg(&cna)
                .args(["--command", name, "-o"])
                .arg(&schema));
            let mut diagnostics = String::from_utf8_lossy(&initialized.stderr).to_string();
            let mut passed = initialized.status.success();
            let mut output_bytes = 0;
            if passed {
                let built = run(converter()
                    .arg("build")
                    .arg(&schema)
                    .arg("-o")
                    .arg(&executable)
                    .arg("--docs")
                    .arg(&docs));
                passed = built.status.success();
                diagnostics.push_str(&String::from_utf8_lossy(&built.stderr));
            }
            if passed {
                // Remove the generated schema and the copied input object before executing.
                let spec = bof2exe::tool::Spec::load(&schema).unwrap();
                let flags: Vec<_> = spec.parameters.iter().map(|p| p.name.clone()).collect();
                fs::remove_file(&schema).unwrap();
                fs::remove_file(&copied_object).unwrap();
                let help = run(Command::new(&executable).arg("--help"));
                let help_text = String::from_utf8_lossy(&help.stdout);
                passed &= help.status.success()
                    && flags
                        .iter()
                        .all(|flag| help_text.contains(&format!("--{flag}")));
                passed &= fs::read_to_string(&docs).unwrap().contains("Usage:");
                let mut invoke = Command::new(&executable);
                if ["md5", "sha256", "cat"].contains(&name) {
                    invoke.arg(format!("--{}", flags[0])).arg(&input);
                }
                if name == "dir" {
                    invoke
                        .arg(format!("--{}", flags[0]))
                        .arg(directory.path())
                        .arg(format!("--{}", flags[1]))
                        .arg("0");
                }
                let output = run(&mut invoke);
                output_bytes = output.stdout.len();
                passed &= output.status.success()
                    && output.stderr.is_empty()
                    && String::from_utf8_lossy(&output.stdout).contains(expected);
                if !passed {
                    diagnostics.push_str(&format!(
                        " status={} stderr={} expected={expected:?} output_bytes={output_bytes}",
                        output.status,
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
            }
            println!("{name}: {}", if passed { "PASS" } else { "FAIL" });
            reports.push(serde_json::json!({"name":name,"passed":passed,"stdout_bytes":output_bytes,"diagnostics":diagnostics}));
        }
        write_report("trustedsec-standalone.json", &reports);
        assert!(
            reports.iter().all(|r| r["passed"] == true),
            "one or more upstream standalone tests failed; see JSON report"
        );
    }
}
