//! Public, portable schema for turning a BOF's ordered wire arguments into a CLI.
use crate::args::Argument;
use anyhow::{ensure, Context, Result};
use clap::{error::ErrorKind, Arg, ArgAction, Command};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema: u32,
    pub name: String,
    #[serde(default = "version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub object: PathBuf,
    #[serde(default = "entry")]
    pub entry: String,
    #[serde(default = "error_exit")]
    pub error_exit: bool,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
}
fn version() -> String {
    "1.0.0".into()
}
fn entry() -> String {
    "go".into()
}
fn error_exit() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default = "error_exit")]
    pub error_exit: bool,
    pub parameters: Vec<Parameter>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<char>,
    #[serde(default)]
    pub help: String,
    #[serde(default = "required")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub positional: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub switch: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Encoding::is_base64")]
    pub encoding: Encoding,
}
fn required() -> bool {
    true
}
fn is_false(value: &bool) -> bool {
    !*value
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Str,
    Wstr,
    Int,
    Short,
    Bin,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
    #[default]
    Base64,
    Hex,
    File,
}
impl Encoding {
    fn is_base64(&self) -> bool {
        *self == Self::Base64
    }
}
impl Kind {
    pub fn from_wire(value: &str) -> Result<Self> {
        Ok(match value {
            "str" => Self::Str,
            "wstr" => Self::Wstr,
            "int" => Self::Int,
            "short" => Self::Short,
            "bin" => Self::Bin,
            _ => anyhow::bail!("unknown wire type {value}"),
        })
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::Str => "str",
            Self::Wstr => "wstr",
            Self::Int => "int",
            Self::Short => "short",
            Self::Bin => "bin",
        }
    }
}
impl Spec {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = crate::read_input(path)?;
        let mut spec: Self = toml::from_str(std::str::from_utf8(&bytes)?)?;
        ensure!(
            spec.schema == 1,
            "unsupported tool schema {}; expected 1",
            spec.schema
        );
        ensure!(
            !spec.entry.is_empty() && !spec.entry.contains('\0'),
            "invalid entrypoint"
        );
        spec.definition().validate()?;
        if spec.object.is_relative() {
            spec.object = path.parent().unwrap_or(Path::new(".")).join(&spec.object);
        }
        Ok(spec)
    }
    pub fn definition(&self) -> Definition {
        Definition {
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            error_exit: self.error_exit,
            parameters: self.parameters.clone(),
        }
    }
}

impl Parameter {
    pub fn new(name: String, kind: Kind) -> Self {
        Self {
            name,
            kind,
            short: None,
            help: String::new(),
            required: true,
            default: None,
            positional: false,
            switch: false,
            choices: Vec::new(),
            encoding: Encoding::Base64,
        }
    }
    fn convert(&self, value: &str) -> Result<Argument> {
        if self.kind == Kind::Bin && self.encoding == Encoding::File {
            return Ok(Argument::Bin(
                crate::read_input(Path::new(value))
                    .with_context(|| format!("read --{} input file {value:?}", self.name))?,
            ));
        }
        let prefix = if self.kind == Kind::Bin && self.encoding == Encoding::Hex {
            "hex"
        } else {
            self.kind.prefix()
        };
        Ok(format!("{prefix}:{value}").parse()?)
    }
    fn implicit(&self) -> Argument {
        match self.kind {
            Kind::Str => Argument::Str(String::new()),
            Kind::Wstr => Argument::Wstr(String::new()),
            Kind::Int => Argument::Int(0),
            Kind::Short => Argument::Short(0),
            Kind::Bin => Argument::Bin(Vec::new()),
        }
    }
}

impl Definition {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.is_empty()
                && self.name.len() <= 128
                && self
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
            "tool name must contain only letters, digits, - or _"
        );
        ensure!(
            !self.version.is_empty() && !self.version.contains(['\0', '\n', '\r']),
            "invalid tool version"
        );
        ensure!(!self.description.contains('\0'), "description contains NUL");
        ensure!(self.parameters.len() <= 1024, "too many tool parameters");
        let mut names = BTreeSet::from(["help".to_owned(), "version".to_owned()]);
        let mut shorts = BTreeSet::from(['h', 'V']);
        let mut optional_position = false;
        for parameter in &self.parameters {
            let name = &parameter.name;
            ensure!(
                !name.is_empty()
                    && name.len() <= 128
                    && name.as_bytes()[0].is_ascii_lowercase()
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
                "invalid parameter name {name:?}; use lower-case words separated by -"
            );
            ensure!(
                names.insert(name.clone()),
                "duplicate or reserved parameter name {name:?}"
            );
            ensure!(
                !parameter.help.contains('\0'),
                "parameter help contains NUL"
            );
            if let Some(short) = parameter.short {
                ensure!(
                    short.is_ascii_alphabetic() && shorts.insert(short),
                    "duplicate, reserved, or invalid short option {short:?}"
                );
            }
            ensure!(
                !parameter.positional || parameter.short.is_none(),
                "positional {name} cannot have a short option"
            );
            ensure!(
                !parameter.switch
                    || (!parameter.positional && matches!(parameter.kind, Kind::Int | Kind::Short)),
                "switch {name} must be a named int or short parameter"
            );
            ensure!(
                parameter.kind == Kind::Bin || parameter.encoding == Encoding::Base64,
                "encoding applies only to binary parameters"
            );
            if parameter.positional {
                let is_required = parameter.required && parameter.default.is_none();
                ensure!(
                    !is_required || !optional_position,
                    "required positionals must precede optional positionals"
                );
                optional_position |= !is_required;
            }
            ensure!(
                parameter.choices.len() <= 256,
                "too many choices for {name}"
            );
            let mut choices = BTreeSet::new();
            for value in &parameter.choices {
                ensure!(
                    choices.insert(value) && !value.is_empty(),
                    "duplicate or empty choice for {name}"
                );
            }
            if parameter.switch {
                ensure!(
                    parameter.choices.is_empty(),
                    "switch {name} cannot define choices"
                );
                if let Some(value) = &parameter.default {
                    ensure!(
                        ["true", "false"].contains(&value.as_str()),
                        "switch default must be true or false"
                    );
                }
            } else {
                ensure!(
                    parameter.required
                        || parameter.choices.is_empty()
                        || parameter.default.is_some(),
                    "optional choice parameter {name} needs an explicit default"
                );
                if let Some(value) = &parameter.default {
                    ensure!(
                        parameter.choices.is_empty() || parameter.choices.contains(value),
                        "default for {name} is not an allowed choice"
                    );
                    if !(parameter.kind == Kind::Bin && parameter.encoding == Encoding::File) {
                        parameter
                            .convert(value)
                            .with_context(|| format!("invalid default for {name}"))?;
                    }
                }
                for value in &parameter.choices {
                    if !(parameter.kind == Kind::Bin && parameter.encoding == Encoding::File) {
                        parameter
                            .convert(value)
                            .with_context(|| format!("invalid choice for {name}"))?;
                    }
                }
            }
        }
        Ok(())
    }

    /// The same command model drives standalone help, parsing, and generated docs.
    pub fn command(&self) -> Result<Command> {
        self.validate()?;
        let mut command = Command::new(self.name.clone())
            .bin_name(self.name.clone())
            .version(self.version.clone())
            .about(self.description.clone());
        let mut position = 1;
        for parameter in &self.parameters {
            let mut argument = Arg::new(parameter.name.clone()).help(parameter.help.clone());
            if parameter.positional {
                argument = argument.index(position);
                position += 1;
            } else {
                argument = argument.long(parameter.name.clone());
                if let Some(short) = parameter.short {
                    argument = argument.short(short);
                }
            }
            if parameter.switch {
                argument = argument
                    .action(ArgAction::Set)
                    .num_args(0..=1)
                    .require_equals(true)
                    .default_missing_value("true")
                    .default_value(parameter.default.clone().unwrap_or_else(|| "false".into()))
                    .value_parser(clap::builder::BoolishValueParser::new())
                    .value_name("BOOL");
            } else {
                argument = argument
                    .action(ArgAction::Set)
                    .required(parameter.required && parameter.default.is_none())
                    .value_name(parameter.name.replace('-', "_").to_ascii_uppercase());
                if let Some(default) = &parameter.default {
                    argument = argument.default_value(default.clone());
                }
                if !parameter.choices.is_empty() {
                    argument = argument.value_parser(clap::builder::PossibleValuesParser::new(
                        parameter.choices.clone(),
                    ));
                }
                if matches!(parameter.kind, Kind::Int | Kind::Short) {
                    argument = argument.allow_negative_numbers(true);
                }
                if parameter.kind == Kind::Bin && parameter.encoding == Encoding::File {
                    argument = argument.value_hint(clap::ValueHint::FilePath);
                }
            }
            command = command.arg(argument);
        }
        Ok(command)
    }

    pub fn parse_from(
        &self,
        args: impl IntoIterator<Item = impl Into<OsString> + Clone>,
    ) -> std::result::Result<Vec<Argument>, clap::Error> {
        let mut command = self
            .command()
            .map_err(|e| clap::Error::raw(ErrorKind::InvalidValue, e.to_string()))?;
        let matches = command.clone().try_get_matches_from(args)?;
        let mut output = Vec::new();
        for parameter in &self.parameters {
            let argument = if parameter.switch {
                let value = i32::from(*matches.get_one::<bool>(&parameter.name).unwrap_or(&false));
                if parameter.kind == Kind::Short {
                    Argument::Short(value as i16)
                } else {
                    Argument::Int(value)
                }
            } else if let Some(value) = matches.get_one::<String>(&parameter.name) {
                parameter.convert(value).map_err(|e| {
                    command.error(
                        ErrorKind::ValueValidation,
                        format!("invalid value for {}: {e:#}", parameter.name),
                    )
                })?
            } else {
                parameter.implicit()
            };
            output.push(argument);
        }
        Ok(output)
    }

    pub fn markdown(&self) -> Result<String> {
        let mut command = self.command()?;
        let help = command.render_long_help();
        Ok(format!(
            "# {}\n\n{}\n\n```text\n{help}\n```\n",
            self.name, self.description
        ))
    }
}

/// Make a reviewable schema from a BOF signature, optionally using CNA usage labels.
pub fn scaffold(
    object: PathBuf,
    name: String,
    format: &str,
    description: String,
    usage: Option<&str>,
) -> Result<Spec> {
    let kinds = crate::args::format_types(format)?;
    let mut candidates = Vec::new();
    if let Some(usage) = usage {
        let mut current = String::new();
        let mut closing = None;
        for c in usage.chars() {
            if closing == Some(c) {
                candidates.push(current.clone());
                current.clear();
                closing = None;
            } else if closing.is_some() {
                current.push(c);
            } else if c == '<' || c == '[' {
                closing = Some(if c == '<' { '>' } else { ']' });
            }
        }
    }
    let mut used = BTreeSet::new();
    let parameters = kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            let candidate = if candidates.len() == kinds.len() {
                sanitize(&candidates[index])
            } else {
                String::new()
            };
            let mut name = if candidate.is_empty()
                || ["help", "version"].contains(&candidate.as_str())
                || used.contains(&candidate)
            {
                format!("arg{}", index + 1)
            } else {
                candidate
            };
            if used.contains(&name) {
                let base = name.clone();
                let mut suffix = 2;
                while used.contains(&name) {
                    name = format!("{base}-{suffix}");
                    suffix += 1;
                }
            }
            used.insert(name.clone());
            let mut param = Parameter::new(name.clone(), Kind::from_wire(kind)?);
            param.help = format!("{} value", name.replace('-', " "));
            Ok(param)
        })
        .collect::<Result<Vec<_>>>()?;
    let spec = Spec {
        schema: 1,
        name,
        version: version(),
        description,
        object,
        entry: entry(),
        error_exit: true,
        parameters,
    };
    spec.definition().validate()?;
    Ok(spec)
}
pub fn sanitize(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.as_bytes().first().is_some_and(u8::is_ascii_lowercase) {
        out
    } else {
        String::new()
    }
}
