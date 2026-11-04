#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use std::{
        path::{Path, PathBuf},
        process::Command,
    };
    fn converter() -> Command {
        Command::new(env!("CARGO_BIN_EXE_bof2exe"))
    }
    fn fixture(directory: &Path) -> PathBuf {
        let output = directory.join("hello.o");
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
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello.rs"))
            .arg("-o")
            .arg(&output)
            .status()
            .unwrap();
        assert!(status.success());
        output
    }
    #[test]
    fn generated_tool_has_named_arguments_and_survives_removing_its_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let object = fixture(directory.path());
        let spec = directory.path().join("hello.tool.toml");
        let executable = directory.path().join("greet.exe");
        let docs = directory.path().join("greet.md");
        let cna = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/hello.cna");
        let output = converter()
            .arg("init")
            .arg(&object)
            .arg("--cna")
            .arg(cna)
            .args(["--command", "hello", "--name", "greet", "-o"])
            .arg(&spec)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut schema = bof2exe::tool::Spec::load(&spec).unwrap();
        assert_eq!(schema.parameters[0].name, "count");
        assert_eq!(schema.parameters[1].name, "name");
        schema.parameters[0].default = Some("7".into());
        schema.parameters[1].help = "Person to greet".into();
        std::fs::write(&spec, toml::to_string(&schema).unwrap()).unwrap();
        let output = converter()
            .arg("build")
            .arg(&spec)
            .arg("-o")
            .arg(&executable)
            .arg("--docs")
            .arg(&docs)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(std::fs::read_to_string(docs)
            .unwrap()
            .contains("Person to greet"));
        std::fs::remove_file(object).unwrap();
        std::fs::remove_file(spec).unwrap();
        let help = Command::new(&executable).arg("--help").output().unwrap();
        assert!(help.status.success());
        let text = String::from_utf8(help.stdout).unwrap();
        assert!(text.contains("--name"));
        assert!(text.contains("--count"));
        assert!(!text.contains("TYPE:VALUE"));
        assert!(!text.contains("bof2exe"));
        let output = Command::new(&executable)
            .args(["--name", "Alice"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("hello Alice: count=7"));
        let output = Command::new(&executable)
            .args(["--count", "3", "--name", "Bob"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("hello Bob: count=3"));
        let invalid = Command::new(&executable)
            .args(["--count", "bad", "--name", "Bob"])
            .output()
            .unwrap();
        assert_eq!(invalid.status.code(), Some(2));
        assert!(invalid.stdout.is_empty());
        assert_eq!(
            Command::new(&executable).output().unwrap().status.code(),
            Some(2)
        );
    }
    #[test]
    fn direct_cna_build_and_callback_errors_use_cli_exit_statuses() {
        let directory = tempfile::tempdir().unwrap();
        let object = fixture(directory.path());
        let executable = directory.path().join("hello.exe");
        let cna = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/hello.cna");
        let output = converter()
            .arg("build")
            .arg(&object)
            .arg("--cna")
            .arg(cna)
            .args(["--command", "hello", "-o"])
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(executable)
            .args(["--count", "4", "--name", "direct"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("hello direct: count=4"));
        let error_tool = directory.path().join("error.exe");
        assert!(converter()
            .arg("build")
            .arg(object)
            .arg("-o")
            .arg(&error_tool)
            .status()
            .unwrap()
            .success());
        let error = Command::new(error_tool).output().unwrap();
        assert_eq!(error.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&error.stderr).contains("expected int and str"));
    }
}
