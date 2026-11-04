use bof2exe::{
    args::Argument,
    tool::{Definition, Encoding, Kind, Parameter, Spec},
};
fn definition(parameters: Vec<Parameter>) -> Definition {
    Definition {
        name: "hello".into(),
        version: "1.2.3".into(),
        description: "Print a greeting".into(),
        error_exit: true,
        parameters,
    }
}

#[test]
fn named_options_defaults_and_wire_order_are_independent_of_command_order() {
    let mut count = Parameter::new("count".into(), Kind::Int);
    count.short = Some('c');
    count.default = Some("7".into());
    let mut name = Parameter::new("name".into(), Kind::Wstr);
    name.short = Some('n');
    let definition = definition(vec![count, name]);
    assert_eq!(
        definition.parse_from(["hello", "--name", "Alice"]).unwrap(),
        [Argument::Int(7), Argument::Wstr("Alice".into())]
    );
    assert_eq!(
        definition
            .parse_from(["hello", "-n", "Bob", "-c", "-2"])
            .unwrap(),
        [Argument::Int(-2), Argument::Wstr("Bob".into())]
    );
    assert_eq!(definition.parse_from(["hello"]).unwrap_err().exit_code(), 2);
    assert_eq!(
        definition
            .parse_from(["hello", "--name", "Bob", "--count", "bad"])
            .unwrap_err()
            .exit_code(),
        2
    );
}
#[test]
fn switches_support_explicit_false_and_optional_fields_still_pack() {
    let mut flag = Parameter::new("recursive".into(), Kind::Short);
    flag.switch = true;
    flag.default = Some("true".into());
    let mut text = Parameter::new("filter".into(), Kind::Str);
    text.required = false;
    let definition = definition(vec![flag, text]);
    assert_eq!(
        definition.parse_from(["hello"]).unwrap(),
        [Argument::Short(1), Argument::Str(String::new())]
    );
    assert_eq!(
        definition
            .parse_from(["hello", "--recursive=false"])
            .unwrap(),
        [Argument::Short(0), Argument::Str(String::new())]
    );
    assert_eq!(
        definition.parse_from(["hello", "--recursive"]).unwrap()[0],
        Argument::Short(1)
    );
}
#[test]
fn positionals_choices_and_binary_file_inputs_are_supported() {
    let mut path = Parameter::new("path".into(), Kind::Wstr);
    path.positional = true;
    let mut mode = Parameter::new("mode".into(), Kind::Str);
    mode.default = Some("fast".into());
    mode.choices = vec!["fast".into(), "slow".into()];
    let mut data = Parameter::new("input".into(), Kind::Bin);
    data.encoding = Encoding::File;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("input.bin");
    std::fs::write(&file, [0, 255, 17]).unwrap();
    let definition = definition(vec![path, mode, data]);
    let values = definition
        .parse_from([
            "hello",
            "C:\\example",
            "--mode",
            "slow",
            "--input",
            file.to_str().unwrap(),
        ])
        .unwrap();
    assert_eq!(
        values,
        [
            Argument::Wstr("C:\\example".into()),
            Argument::Str("slow".into()),
            Argument::Bin(vec![0, 255, 17])
        ]
    );
    assert_eq!(
        definition
            .parse_from(["hello", "C:\\example", "--mode", "unknown"])
            .unwrap_err()
            .exit_code(),
        2
    );
}
#[test]
fn invalid_schemas_are_rejected_before_clap_can_panic() {
    for name in ["help", "version", "Bad Name", "-bad"] {
        assert!(
            definition(vec![Parameter::new(name.into(), Kind::Str)])
                .validate()
                .is_err(),
            "{name}"
        );
    }
    let mut short = Parameter::new("name".into(), Kind::Str);
    short.short = Some('h');
    assert!(definition(vec![short]).validate().is_err());
    let duplicate = Parameter::new("name".into(), Kind::Str);
    assert!(definition(vec![duplicate.clone(), duplicate])
        .validate()
        .is_err());
    let mut switch = Parameter::new("name".into(), Kind::Str);
    switch.switch = true;
    assert!(definition(vec![switch]).validate().is_err());
    let mut bad = Parameter::new("count".into(), Kind::Short);
    bad.default = Some("99999".into());
    assert!(definition(vec![bad]).validate().is_err());
}
#[test]
fn help_version_and_reference_are_specific_to_the_generated_command() {
    let mut name = Parameter::new("name".into(), Kind::Str);
    name.help = "Person to greet".into();
    let definition = definition(vec![name]);
    let help = definition.parse_from(["hello", "--help"]).unwrap_err();
    assert_eq!(help.exit_code(), 0);
    let text = help.to_string();
    assert!(text.contains("Person to greet"));
    assert!(text.contains("--name"));
    assert!(!text.contains("bof2exe"));
    assert!(definition
        .parse_from(["hello", "--version"])
        .unwrap_err()
        .to_string()
        .contains("1.2.3"));
    assert!(definition.markdown().unwrap().contains("Usage: hello"));
}
#[test]
fn scaffold_uses_usage_names_and_schema_paths_are_relative() {
    let spec = bof2exe::tool::scaffold(
        "hello.o".into(),
        "hello".into(),
        "iz",
        "Greeting".into(),
        Some("hello <count> <name>"),
    )
    .unwrap();
    assert_eq!(spec.parameters[0].name, "count");
    assert_eq!(spec.parameters[1].name, "name");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tool.toml");
    std::fs::write(&path, toml::to_string(&spec).unwrap()).unwrap();
    assert_eq!(
        Spec::load(&path).unwrap().object,
        directory.path().join("hello.o")
    );
}
