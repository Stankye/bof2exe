mod support;

#[cfg(all(windows, target_arch = "x86_64"))]
mod native {
    use std::{
        path::{Path, PathBuf},
        process::Command,
    };
    fn cli() -> Command {
        Command::new(env!("CARGO_BIN_EXE_bof2exe"))
    }
    fn compile(name: &str, output: &Path) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let object = output.join(format!("{name}.obj"));
        let tool = find_msvc_tools::find_tool("x86_64-pc-windows-msvc", "cl.exe")
            .expect("Windows tests require the documented C++ Build Tools");
        let result = tool
            .to_command()
            .args([
                "/nologo", "/c", "/O1", "/Ob0", "/Gy", "/GS-", "/Zl", "/GR-", "/EHs-c-", "/bigobj",
            ])
            .arg(format!("/I{}", root.join("include").display()))
            .arg(root.join("tests/fixtures").join(format!("{name}.cpp")))
            .arg(format!("/Fo{}", object.display()))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        object
    }
    #[test]
    fn msvc_archives_comdat_and_zip_libraries_execute_real_compiler_output() {
        let directory = tempfile::tempdir().unwrap();
        let root = compile("comdat_main", directory.path());
        let first = compile("comdat_first", directory.path());
        let second = compile("comdat_second", directory.path());
        let library = directory.path().join("helpers.lib");
        let tool =
            find_msvc_tools::find_tool("x86_64-pc-windows-msvc", "lib.exe").expect("MSVC lib.exe");
        let result = tool
            .to_command()
            .arg("/nologo")
            .arg(format!("/OUT:{}", library.display()))
            .arg(&first)
            .arg(&second)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let zip = directory.path().join("helpers.zip");
        let a = std::fs::read(first).unwrap();
        let b = std::fs::read(second).unwrap();
        std::fs::write(
            &zip,
            super::support::zip(&[("first.obj", &a), ("second.obj", &b)], true),
        )
        .unwrap();
        for (index, library) in [library, zip].iter().enumerate() {
            let linked = directory.path().join(format!("linked{index}.o"));
            let result = cli()
                .arg("link")
                .arg(&root)
                .arg("--library")
                .arg(library)
                .arg("-o")
                .arg(&linked)
                .arg("--json")
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(report["libraries"]["selected"].as_array().unwrap().len(), 2);
            assert!(!report["link"]["comdat_discarded"]
                .as_array()
                .unwrap()
                .is_empty());
            assert!(
                report["compatibility"]["issues"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "{report}"
            );
            let result = cli().arg("run").arg(linked).output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                String::from_utf8(result.stdout).unwrap(),
                "first=21 second=24\n"
            );
        }
    }
    #[test]
    fn suite_execution_uses_workers_and_verifies_native_output() {
        let directory = tempfile::tempdir().unwrap();
        let root = compile("comdat_main", directory.path());
        let first = compile("comdat_first", directory.path());
        let second = compile("comdat_second", directory.path());
        let linked = directory.path().join("linked.o");
        let result = cli()
            .arg("link")
            .arg(root)
            .arg(first)
            .arg(second)
            .arg("-o")
            .arg(&linked)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let suite = directory.path().join("suite.toml");
        std::fs::write(&suite,"schema=1\nname='native'\n[[cases]]\nname='output'\nobject='linked.o'\n[[cases.expect]]\nkind=0\ncontains='first=21 second=24'\n[[cases]]\nname='entrypoint'\nobject='linked.o'\nentry='missing'\ncompatible=false\nissue_contains='entrypoint'\n").unwrap();
        let result = cli()
            .arg("suite")
            .arg(suite)
            .args(["--execute", "--json"])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["passed"], 2);
        assert_eq!(report["failed"], 0);
        assert_eq!(report["cases"][0]["executed"], true);
        assert_eq!(report["cases"][1]["executed"], false);
    }
}
