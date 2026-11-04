use std::process::Command;
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bof2exe"))
}

#[test]
fn typed_pack_roundtrip_and_validation() {
    let output = cli()
        .args(["pack", "--", "int:42", "str:ok"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "CwAAACoAAAADAAAAb2sA"
    );
    let output = cli().args(["pack", "--", "short:99999"]).output().unwrap();
    assert!(!output.status.success());
}
#[test]
fn docs_and_completions_come_from_the_cli() {
    let directory = tempfile::tempdir().unwrap();
    let status = cli()
        .arg("docs")
        .arg("--output")
        .arg(directory.path())
        .status()
        .unwrap();
    assert!(status.success());
    let markdown = std::fs::read_to_string(directory.path().join("cli.md")).unwrap();
    assert!(markdown.contains("bof2exe bundle"));
    assert!(!markdown.contains("__worker"));
    assert!(directory.path().join("bof2exe-run.1").exists());
    for shell in ["bash", "power-shell"] {
        let output = cli().args(["completions", shell]).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("--manifest"));
    }
}
#[test]
fn output_files_are_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("args.bin");
    std::fs::write(&path, b"keep").unwrap();
    assert!(!cli()
        .arg("pack")
        .arg("-o")
        .arg(&path)
        .status()
        .unwrap()
        .success());
    assert_eq!(std::fs::read(&path).unwrap(), b"keep");
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn fixture_named(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let object = directory.path().join("hello.o");
    let status = Command::new("rustc")
        .args([
            "--edition=2021",
            "--crate-type",
            "lib",
            "--emit=obj",
            "-C",
            "panic=abort",
            "-C",
            "opt-level=2",
            "-C",
            "overflow-checks=off",
        ])
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(format!("{name}.rs")),
        )
        .arg("-o")
        .arg(&object)
        .status()
        .unwrap();
    assert!(status.success());
    (directory, object)
}
#[test]
#[cfg(all(windows, target_arch = "x86_64"))]
fn run_and_standalone_bundle_execute_a_real_object() {
    let (directory, object) = fixture_named("hello");
    let check = cli()
        .arg("check")
        .arg(&object)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&check.stdout).unwrap();
    assert!(report["issues"].as_array().unwrap().is_empty());
    let output = cli()
        .arg("run")
        .arg(&object)
        .args(["--", "int:7", "str:world"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("hello world: count=7 bss=1 pid="), "{text}");
    assert!(text.contains("extra=17 23"), "{text}");
    let cna = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/hello.cna");
    let inferred = cli()
        .arg("run")
        .arg(&object)
        .arg("--cna")
        .arg(cna)
        .args(["--command", "hello", "--", "42", "cna"])
        .output()
        .unwrap();
    assert!(
        inferred.status.success(),
        "{}",
        String::from_utf8_lossy(&inferred.stderr)
    );
    assert!(String::from_utf8(inferred.stdout)
        .unwrap()
        .contains("hello cna: count=42"));
    let executable = directory.path().join("hello.exe");
    let status = cli()
        .arg("bundle")
        .arg(&object)
        .arg("-o")
        .arg(&executable)
        .args(["--", "int:8", "str:bundle"])
        .status()
        .unwrap();
    assert!(status.success());
    // Remove the source to prove that the generated executable is self-contained.
    std::fs::remove_file(&object).unwrap();
    let output = Command::new(&executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("hello bundle: count=8 bss=1"));
    let output = Command::new(&executable)
        .args(["--", "int:9", "str:override"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("hello override: count=9 bss=1"));
}

#[test]
#[cfg(all(windows, target_arch = "x86_64"))]
fn worker_timeout_and_native_fault_are_reported() {
    let (_dir, object) = fixture_named("spin");
    let start = std::time::Instant::now();
    let output = cli()
        .arg("run")
        .arg(&object)
        .args(["--timeout", "1"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("deadline"));
    assert!(start.elapsed() < std::time::Duration::from_secs(10));
    let (_dir, object) = fixture_named("fault");
    let output = cli().arg("run").arg(&object).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("worker failed"));
}

#[test]
#[cfg(all(windows, target_arch = "x86_64"))]
fn ordinary_windows_loader_imports_resolve_in_a_real_object() {
    let (_directory, object) = fixture_named("loader_imports");
    let output = cli().arg("run").arg(object).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"loader-imports=ok\n");
}

#[test]
#[cfg(all(windows, target_arch = "x86_64"))]
fn stack_probes_crt_helpers_and_unwind_registration_work() {
    let (_dir, object) = fixture_named("stack");
    let output = cli()
        .arg("run")
        .arg(&object)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events: Vec<bof2exe::runtime::Output> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(events[0].data.len(), 16384);
    assert_eq!(events[0].data[0], 17);
    assert_eq!(events[0].data[16383], 23);
    let (_dir, object) = fixture_named("unwind");
    let output = cli().arg("run").arg(object).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "unwind=1\n");
}

#[test]
#[cfg(all(windows, target_arch = "x86_64"))]
fn composed_objects_and_resource_execute_and_bundle() {
    let (directory, main) = fixture_named("link_main");
    let (_helper_dir, helper) = fixture_named("link_helper");
    std::fs::write(directory.path().join("message.txt"), b"resource-content").unwrap();
    let spec = directory.path().join("build.toml");
    let manifest = serde_json::json!({"schema":1,"name":"composed","objects":[main,helper],"resources":[{"path":"message.txt","symbol":"greeting"}]});
    let table: toml::Value = serde_json::from_value(manifest).unwrap();
    std::fs::write(&spec, toml::to_string(&table).unwrap()).unwrap();
    let linked = directory.path().join("linked.o");
    let output = cli()
        .arg("link")
        .arg("--spec")
        .arg(&spec)
        .arg("-o")
        .arg(&linked)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["compatibility"]["issues"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{report}"
    );
    let output = cli().arg("run").arg(&linked).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "linked=22 data=resource-content"
    );
    let exe = directory.path().join("composed.exe");
    assert!(cli()
        .arg("bundle")
        .arg(&linked)
        .arg("-o")
        .arg(&exe)
        .status()
        .unwrap()
        .success());
    let output = std::process::Command::new(exe).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "linked=22 data=resource-content"
    );
}
