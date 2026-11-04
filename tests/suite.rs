mod support;
use bof2exe::{
    runtime::Output,
    suite::{Case, Expectation, Suite},
};
use support::{object, Section, Symbol};

#[test]
fn inspect_mode_defers_output_without_calling_the_executor() {
    let directory = tempfile::tempdir().unwrap();
    let bytes = object(
        0x8664,
        false,
        &[Section::text(vec![0xc3])],
        &[Symbol::external("go", 1, 0)],
    );
    std::fs::write(directory.path().join("test.o"), bytes).unwrap();
    let path = directory.path().join("suite.toml");
    std::fs::write(&path,"schema=1\nname='fixture'\n[[cases]]\nname='hello'\nobject='test.o'\n[[cases.expect]]\ncontains='ok'\n").unwrap();
    let suite = Suite::load(&path).unwrap();
    let report = bof2exe::suite::run(&suite, false, |_, _, _, _| {
        panic!("inspection must not execute")
    });
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 0);
    assert!(!report.cases[0].executed);
    assert!(report.cases[0].diagnostics[0].contains("deferred"));
    assert!(report.cases[0].sha256.is_some());
}
#[test]
fn output_expectations_preserve_kind_order_and_binary_bytes() {
    let case = Case {
        name: "binary".into(),
        object: "unused".into(),
        entry: "go".into(),
        arguments: Vec::new(),
        timeout: 1,
        compatible: true,
        issue_contains: None,
        expect: vec![
            Expectation {
                kind: Some(0),
                contains: None,
                exact_base64: Some("AP8=".into()),
            },
            Expectation {
                kind: Some(13),
                contains: Some("error".into()),
                exact_base64: None,
            },
        ],
    };
    let output = [
        Output {
            kind: 0,
            data: vec![0],
        },
        Output {
            kind: 13,
            data: b"error".to_vec(),
        },
        Output {
            kind: 0,
            data: vec![255],
        },
    ];
    assert!(case.verify(&output).is_ok());
    assert!(case.verify(&output[..2]).is_err());
}
#[test]
fn suite_records_failures_and_continues_to_later_cases() {
    let directory = tempfile::tempdir().unwrap();
    let bytes = object(
        0x8664,
        false,
        &[Section::text(vec![0xc3])],
        &[Symbol::external("go", 1, 0)],
    );
    std::fs::write(directory.path().join("test.o"), bytes).unwrap();
    let path = directory.path().join("suite.toml");
    std::fs::write(&path,"schema=1\nname='fixture'\n[[cases]]\nname='missing'\nobject='missing.o'\n[[cases]]\nname='known-incompatible'\nobject='test.o'\nentry='absent'\ncompatible=false\nissue_contains='entrypoint'\n").unwrap();
    let suite = Suite::load(&path).unwrap();
    let report = bof2exe::suite::run(&suite, false, |_, _, _, _| unreachable!());
    assert_eq!(report.failed, 1);
    assert_eq!(report.passed, 1);
    assert!(!report.cases[0].passed);
    assert!(report.cases[1].passed);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bof2exe"))
        .arg("suite")
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failed"], 1);
    assert_eq!(report["cases"].as_array().unwrap().len(), 2);
}
#[test]
fn executor_errors_and_output_mismatches_are_reported() {
    let directory = tempfile::tempdir().unwrap();
    let bytes = object(
        0x8664,
        false,
        &[Section::text(vec![0xc3])],
        &[Symbol::external("go", 1, 0)],
    );
    std::fs::write(directory.path().join("test.o"), bytes).unwrap();
    let path = directory.path().join("suite.toml");
    std::fs::write(&path,"schema=1\nname='fixture'\n[[cases]]\nname='output'\nobject='test.o'\n[[cases.expect]]\ncontains='expected'\n").unwrap();
    let suite = Suite::load(&path).unwrap();
    let report = bof2exe::suite::run(&suite, true, |_, _, _, _| anyhow::bail!("worker timeout"));
    assert_eq!(report.failed, 1);
    assert!(report.cases[0].diagnostics[0].contains("worker timeout"));
    assert!(report.cases[0].executed);
    let report = bof2exe::suite::run(&suite, true, |_, _, _, _| {
        Ok(vec![Output {
            kind: 0,
            data: b"different".to_vec(),
        }])
    });
    assert_eq!(report.failed, 1);
    assert!(report.cases[0].diagnostics[0].contains("absent"));
}
#[test]
fn suite_rejects_ambiguous_or_invalid_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("suite.toml");
    for text in ["schema=99\nname='x'\ncases=[]","schema=1\nname='x'\ncases=[]","schema=1\nname='x'\n[[cases]]\nname='a'\nobject='a.o'\ntimeout=0","schema=1\nname='x'\n[[cases]]\nname='a'\nobject='a.o'\n[[cases.expect]]\nexact_base64='??'","schema=1\nname='x'\n[[cases]]\nname='a'\nobject='a.o'\n[[cases]]\nname='a'\nobject='b.o'"]{std::fs::write(&path,text).unwrap();assert!(Suite::load(&path).is_err(),"{text}");}
}
