use bof2exe::{
    args::{from_format, pack, Argument},
    cna::analyze,
};

#[test]
fn null_or_omitted_inline_arguments_allow_no_parameter_commands_only() {
    let report = analyze(
        r#"alias hostname {
            beacon_inline_execute($1, readbof($1, "hostname"), "go", $null);
        }"#,
        "no-args.cna",
    )
    .unwrap();
    assert!(report.commands[0].no_arguments);
    assert_eq!(report.signature("hostname", None).unwrap(), "");
    assert!(report.signature("hostname", Some(1)).is_err());
    let omitted = analyze(
        r#"alias hostname { beacon_inline_execute($1, readbof($1, "hostname", $null), "go"); }"#,
        "omitted.cna",
    )
    .unwrap();
    assert_eq!(omitted.signature("hostname", None).unwrap(), "");
    assert!(omitted
        .diagnostics
        .iter()
        .any(|d| d.contains("omits its argument buffer")));
    for body in [
        "helper($1);",
        "beacon_inline_execute($1, $obj, \"go\", $arguments);",
        "beacon_inline_execute($1, $obj, \"go\", $null); beacon_inline_execute($1, $obj, \"go\", $arguments);",
        "bof_pack($1, \"z\", $2); beacon_inline_execute($1, $obj, \"go\", $null);",
    ] {
        let report = analyze(&format!("alias demo {{ {body} }}"), "review.cna").unwrap();
        assert!(!report.commands[0].no_arguments, "{body}");
        if report.commands[0].packs.is_empty() {
            assert!(report.signature("demo", None).is_err(), "{body}");
        }
    }
}

#[test]
fn extracts_registration_signature_expressions_and_object_hints() {
    let source = r#"
    beacon_command_register("hello", "Say hello", "hello <count> <name>");
    alias hello {
        local('$args $fmt');
        $fmt = "iz";
        $args = bof_pack($1, $fmt, int($2), replace($3, ",", ":"));
        $data = readbof($1, "hello");
        beacon_inline_execute($1, $data, "go", $args);
    }"#;
    let report = analyze(source, "fixture.cna").unwrap();
    assert_eq!(report.signature("hello", None).unwrap(), "iz");
    let command = &report.commands[0];
    assert_eq!(command.description.as_deref(), Some("Say hello"));
    assert_eq!(command.usage.as_deref(), Some("hello <count> <name>"));
    assert_eq!(
        command.packs[0].parameters[1].expression,
        r#"replace($3, ",", ":")"#
    );
    assert_eq!(command.packs[0].parameters[1].wire_type, "str");
    assert_eq!(command.entrypoints, ["go"]);
    assert!(command.object_hints.iter().any(|v| v == "\"hello\""));
    assert!(command.calls.contains(&"replace".into()));
}
#[test]
fn ambiguous_branches_require_a_variant() {
    let report = analyze(r#"alias demo { if ($2) { $a = bof_pack($1, "i", $2); } else { $a = bof_pack($1, "z", $2); } }"#, "test").unwrap();
    assert!(report.signature("demo", None).is_err());
    assert_eq!(report.signature("demo", Some(1)).unwrap(), "i");
    assert_eq!(report.signature("demo", Some(2)).unwrap(), "z");
    assert!(report.signature("demo", Some(0)).is_err());
    assert!(report.signature("demo", Some(3)).is_err());
}
#[test]
fn dynamic_or_conditional_formats_are_never_guessed() {
    for body in [
        r#"$a = bof_pack($1, $dynamic, $2);"#,
        r#"$a = bof_pack($1, $fmt, $2); $fmt = "i";"#,
        r#"if ($2) { $fmt = "i"; } $a = bof_pack($1, $fmt, $2);"#,
        r#"if ($2) $fmt = "i"; $a = bof_pack($1, $fmt, $2);"#,
        r#"$fmt = "i"; $fmt = "z"; $a = bof_pack($1, $fmt, $2);"#,
        r#"$fmt = "i"; $fmt .= "z"; $a = bof_pack($1, $fmt, $2);"#,
        r#"$fmt="i";$fmt.="z";$a=bof_pack($1,$fmt,$2);"#,
        r#"$fmt="i";$fmt++;$a=bof_pack($1,$fmt,$2);"#,
        r#"$a = bof_pack($1, "i$dynamic", $2);"#,
    ] {
        let report = analyze(&format!("alias demo {{ {body} }}"), "test").unwrap();
        assert!(report.signature("demo", None).is_err(), "{body}");
        assert!(report.commands[0].packs[0].format.is_none(), "{body}");
    }
}
#[test]
fn comments_and_quoted_code_do_not_create_calls() {
    let report = analyze(
        r#"
      # alias fake { bof_pack($1, "i", $2); }
      /* alias fake { } */
      alias real {
        $s = 'bof_pack($1, "i", $2)'; // ignored
        $a = &bof_pack($1, 'z', "text # } (, \\");
      }
    "#,
        "test",
    )
    .unwrap();
    assert_eq!(report.commands.len(), 1);
    assert_eq!(report.commands[0].packs.len(), 1);
    assert_eq!(report.signature("real", None).unwrap(), "z");
}
#[test]
fn helpers_and_includes_remain_explicit() {
    let report = analyze(r#"include("missing.cna"); sub worker { $a=bof_pack($1,"i",$2); } alias demo { worker($1,$2); }"#, "test").unwrap();
    assert!(report.signature("demo", None).is_err());
    assert_eq!(report.signature("worker", None).unwrap(), "i");
    assert!(report.commands[1].calls.contains(&"worker".into()));
    assert!(report.diagnostics.iter().any(|v| v.contains("included")));
}
#[test]
fn malformed_cna_and_invalid_formats_fail_closed() {
    for source in [
        "alias demo {",
        "alias demo { (] }",
        "/* unterminated",
        "alias demo { $x=\"unterminated; }",
    ] {
        assert!(analyze(source, "test").is_err());
    }
    for source in [
        r#"alias demo { bof_pack($1,"X",$2); }"#,
        r#"alias demo { bof_pack($1,"iz",$2); }"#,
    ] {
        let report = analyze(source, "test").unwrap();
        assert!(report.signature("demo", None).is_err());
    }
}
#[test]
fn format_values_match_typed_wire_bytes() {
    let values = ["-42", "-2", "name", "wide", "hex:00ff"].map(String::from);
    let inferred = from_format("iszZb", &values).unwrap();
    let typed = [
        Argument::Int(-42),
        Argument::Short(-2),
        Argument::Str("name".into()),
        Argument::Wstr("wide".into()),
        Argument::Bin(vec![0, 255]),
    ];
    assert_eq!(pack(&inferred).unwrap(), pack(&typed).unwrap());
    assert!(from_format("ii", &["1".into()]).is_err());
    assert!(from_format("s", &["99999".into()]).is_err());
    assert!(from_format("x", &["1".into()]).is_err());
}
#[test]
fn cna_cli_packs_and_documents_without_executing_the_script() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/hello.cna");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bof2exe"))
        .args(["pack", "--cna"])
        .arg(&path)
        .args(["--command", "hello", "--", "42", "ok"])
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
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bof2exe"))
        .arg("cna")
        .arg(path)
        .arg("--docs")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("| Position | Wire type | CNA expression |"));
    assert!(text.contains("hello <count> <name>"));
}
