//! Static CNA metadata analysis. This module never evaluates Sleep/Aggressor code.
//! Dynamic formats, branches, helpers and transformations remain visible in reports.
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{collections::BTreeMap, ops::Range, path::Path};

#[derive(Debug, Clone, Serialize)]
pub struct Parameter {
    pub position: usize,
    pub wire_type: String,
    pub expression: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct PackCall {
    pub line: usize,
    pub format_expression: String,
    pub format: Option<String>,
    pub parameters: Vec<Parameter>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Command {
    pub name: String,
    pub kind: String,
    pub line: usize,
    pub description: Option<String>,
    pub usage: Option<String>,
    pub object_hints: Vec<String>,
    pub entrypoints: Vec<String>,
    pub calls: Vec<String>,
    pub packs: Vec<PackCall>,
    /// Every direct inline execution supplies `$null` or omits the argument buffer.
    pub no_arguments: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub source: String,
    pub commands: Vec<Command>,
    pub diagnostics: Vec<String>,
}
impl Report {
    /// Select a unique literal signature. A variant is a one-based pack-call index.
    pub fn signature(&self, command: &str, variant: Option<usize>) -> Result<&str> {
        let matches: Vec<_> = self.commands.iter().filter(|c| c.name == command).collect();
        ensure!(
            matches.len() == 1,
            "expected one CNA command {command:?}; found {}",
            matches.len()
        );
        let command = matches[0];
        if command.packs.is_empty() && command.no_arguments && variant.is_none() {
            return Ok("");
        }
        let call = if let Some(index) = variant {
            ensure!(index > 0, "CNA variants are one-based");
            command
                .packs
                .get(index - 1)
                .context("CNA variant is out of range")?
        } else {
            ensure!(
                command.packs.len() == 1,
                "command has {} pack calls; select --variant or inspect helper commands with `cna`",
                command.packs.len()
            );
            &command.packs[0]
        };
        ensure!(
            call.diagnostics.is_empty(),
            "CNA pack call requires review: {}",
            call.diagnostics.join("; ")
        );
        call.format
            .as_deref()
            .context("CNA format is dynamic; specify an explicit --format after reviewing it")
    }
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    literal: Option<String>,
    span: Range<usize>,
    line: usize,
}
fn tokenize(source: &str) -> Result<Vec<Token>> {
    ensure!(source.len() <= crate::MAX_INPUT, "CNA exceeds 64 MiB");
    let bytes = source.as_bytes();
    let (mut i, mut line) = (0, 1);
    let mut out = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            line += usize::from(b == b'\n');
            i += 1;
            continue;
        }
        if b == b'#' || bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            i += 2;
            while i + 1 < bytes.len() && !bytes[i..].starts_with(b"*/") {
                line += usize::from(bytes[i] == b'\n');
                i += 1;
            }
            ensure!(
                i + 1 < bytes.len(),
                "unterminated CNA comment at line {line}"
            );
            i += 2;
            continue;
        }
        let (start, start_line) = (i, line);
        let mut literal = None;
        if b == b'\'' || b == b'"' {
            let quote = b;
            i += 1;
            let mut value = String::new();
            let mut static_value = true;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' {
                    i += 1;
                    ensure!(i < bytes.len(), "unterminated escape at line {line}");
                    match bytes[i] {
                        b'n' => value.push('\n'),
                        b'r' => value.push('\r'),
                        b't' => value.push('\t'),
                        b'\\' => value.push('\\'),
                        b'\'' => value.push('\''),
                        b'"' => value.push('"'),
                        b'$' => value.push('$'),
                        b'%' => value.push('%'),
                        _ => {
                            static_value = false;
                            value.push('\\');
                            value.push(bytes[i] as char);
                        }
                    }
                    i += 1;
                } else {
                    let c = source[i..].chars().next().context("invalid string")?;
                    if quote == b'"' && (c == '$' || c == '%') {
                        static_value = false;
                    }
                    line += usize::from(c == '\n');
                    value.push(c);
                    i += c.len_utf8();
                }
            }
            ensure!(
                i < bytes.len(),
                "unterminated CNA string at line {start_line}"
            );
            i += 1;
            if static_value {
                literal = Some(value);
            }
        } else if (b == b'$' || b == b'%')
            && bytes
                .get(i + 1)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
        } else if b"(){}[],;=".contains(&b) {
            i += 1;
        } else {
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && !b"(){}[],;=\"'".contains(&bytes[i])
            {
                if bytes[i] == b'#'
                    || bytes[i..].starts_with(b"/*")
                    || bytes[i..].starts_with(b"//")
                {
                    break;
                }
                i += 1;
            }
        }
        ensure!(i > start, "unrecognized CNA token at line {line}");
        out.push(Token {
            text: source[start..i].into(),
            literal,
            span: start..i,
            line: start_line,
        });
        ensure!(out.len() <= 500_000, "CNA token count exceeds 500000");
    }
    Ok(out)
}
fn matching(tokens: &[Token]) -> Result<Vec<Option<usize>>> {
    let mut matches = vec![None; tokens.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        if ["(", "[", "{"].contains(&token.text.as_str()) {
            stack.push(i);
            ensure!(stack.len() <= 256, "CNA nesting exceeds 256");
        } else if let Some(pos) = [")", "]", "}"].iter().position(|v| *v == token.text) {
            let start = stack
                .pop()
                .with_context(|| format!("unmatched {} at line {}", token.text, token.line))?;
            ensure!(
                tokens[start].text == ["(", "[", "{"][pos],
                "mismatched CNA delimiters at line {}",
                token.line
            );
            matches[start] = Some(i);
            matches[i] = Some(start);
        }
    }
    ensure!(stack.is_empty(), "unclosed CNA delimiter");
    Ok(matches)
}
fn arguments(tokens: &[Token], matches: &[Option<usize>], open: usize) -> Vec<Range<usize>> {
    let end = matches[open].unwrap_or(open);
    let mut i = open + 1;
    let mut start = i;
    let mut out = Vec::new();
    while i < end {
        if tokens[i].text == "," {
            out.push(start..i);
            start = i + 1;
        } else if let Some(close) = matches[i].filter(|&c| c > i) {
            i = close;
        }
        i += 1;
    }
    if start < end {
        out.push(start..end);
    }
    out
}
fn text(source: &str, tokens: &[Token], range: &Range<usize>) -> String {
    if range.is_empty() {
        return String::new();
    }
    source[tokens[range.start].span.start..tokens[range.end - 1].span.end]
        .trim()
        .into()
}
fn constant(
    tokens: &[Token],
    range: &Range<usize>,
    constants: &BTreeMap<String, String>,
) -> Option<String> {
    if range.len() == 1 {
        return tokens[range.start]
            .literal
            .clone()
            .or_else(|| constants.get(&tokens[range.start].text).cloned());
    }
    None
}

/// Analyze a CNA file without loading includes or executing any code.
pub fn analyze_file(path: &Path) -> Result<Report> {
    let bytes = crate::read_input(path)?;
    analyze(std::str::from_utf8(&bytes)?, &path.display().to_string())
}
pub fn analyze(source: &str, name: &str) -> Result<Report> {
    let tokens = tokenize(source)?;
    let matches = matching(&tokens)?;
    let mut help = BTreeMap::new();
    for i in 0..tokens.len().saturating_sub(1) {
        if tokens[i].text.trim_start_matches('&') == "beacon_command_register"
            && tokens[i + 1].text == "("
        {
            let args = arguments(&tokens, &matches, i + 1);
            if args.len() >= 3 {
                if let (Some(name), Some(description), Some(usage)) = (
                    constant(&tokens, &args[0], &BTreeMap::new()),
                    constant(&tokens, &args[1], &BTreeMap::new()),
                    constant(&tokens, &args[2], &BTreeMap::new()),
                ) {
                    help.insert(name, (description, usage));
                }
            }
        }
    }
    let mut report = Report {
        source: name.into(),
        commands: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut i = 0;
    while i + 2 < tokens.len() {
        if ["alias", "sub"].contains(&tokens[i].text.as_str()) && tokens[i + 2].text == "{" {
            let end = matches[i + 2].context("missing body terminator")?;
            let name = tokens[i + 1]
                .literal
                .clone()
                .unwrap_or_else(|| tokens[i + 1].text.clone());
            let registration = help.get(&name);
            let mut command = Command {
                name,
                kind: tokens[i].text.clone(),
                line: tokens[i].line,
                description: registration.map(|v| v.0.clone()),
                usage: registration.map(|v| v.1.clone()),
                object_hints: Vec::new(),
                entrypoints: Vec::new(),
                calls: Vec::new(),
                packs: Vec::new(),
                no_arguments: false,
            };
            let mut inline_arguments = Vec::new();
            let mut constants = BTreeMap::new();
            let mut assigned = BTreeMap::<String, usize>::new();
            // Only a single, unconditional literal assignment may establish a constant.
            let mut depth = 0usize;
            for j in i + 3..end {
                if tokens[j].text == "{" {
                    depth += 1;
                }
                if tokens[j].text == "}" {
                    depth = depth.saturating_sub(1);
                }
                if tokens[j].text.starts_with('$')
                    && tokens.get(j + 1).is_some_and(|t| t.text == "=")
                {
                    *assigned.entry(tokens[j].text.clone()).or_default() += 1;
                    if depth == 0
                        && (j == i + 3 || tokens[j - 1].text == ";")
                        && tokens.get(j + 3).is_some_and(|t| t.text == ";")
                    {
                        if let Some(value) = tokens.get(j + 2).and_then(|t| t.literal.clone()) {
                            constants.insert(tokens[j].text.clone(), value);
                        }
                    }
                }
            }
            constants.retain(|key, _| assigned.get(key) == Some(&1));
            // Compound assignments and increment/decrement invalidate literal facts.
            constants.retain(|variable, _| {
                !(i + 3..end).any(|j| {
                    (tokens[j].text == *variable
                        && tokens.get(j + 1).is_some_and(|t| {
                            [".", "+", "-", "*", "/", "$+"].contains(&t.text.as_str())
                        })
                        && tokens.get(j + 2).is_some_and(|t| t.text == "="))
                        || (tokens[j].text == *variable
                            && tokens
                                .get(j + 1)
                                .is_some_and(|t| t.text == "++" || t.text == "--"))
                        || tokens[j].text == format!("{variable}++")
                        || tokens[j].text == format!("{variable}--")
                })
            });
            for j in i + 3..end.saturating_sub(1) {
                if tokens[j + 1].text != "(" || tokens[j].literal.is_some() {
                    continue;
                }
                let args = arguments(&tokens, &matches, j + 1);
                match tokens[j].text.trim_start_matches('&') {
                    "bof_pack" => {
                        if args.len() < 2 {
                            report.diagnostics.push(format!(
                                "line {}: bof_pack has fewer than two arguments",
                                tokens[j].line
                            ));
                            continue;
                        }
                        let mut format = constant(&tokens, &args[1], &constants);
                        if args[1].len() == 1 && tokens[args[1].start].literal.is_none() {
                            // A format assignment after this call cannot determine its value here.
                            let variable = &tokens[args[1].start].text;
                            let before = (i + 3..j).any(|k| {
                                &tokens[k].text == variable
                                    && tokens.get(k + 1).is_some_and(|t| t.text == "=")
                            });
                            if !before {
                                format = None;
                            }
                        }
                        let mut diagnostics = Vec::new();
                        let types = if let Some(value) = &format {
                            match crate::args::format_types(value) {
                                Ok(v) => v,
                                Err(e) => {
                                    diagnostics.push(e.to_string());
                                    Vec::new()
                                }
                            }
                        } else {
                            diagnostics
                                .push("format cannot be resolved to a unique literal".into());
                            Vec::new()
                        };
                        if format.is_some() && types.len() != args.len() - 2 {
                            diagnostics.push(format!(
                                "format has {} fields but call supplies {} values",
                                types.len(),
                                args.len() - 2
                            ));
                        }
                        let parameters = args
                            .iter()
                            .skip(2)
                            .enumerate()
                            .map(|(n, arg)| Parameter {
                                position: n + 1,
                                wire_type: types.get(n).unwrap_or(&"unknown").to_string(),
                                expression: text(source, &tokens, arg),
                            })
                            .collect();
                        command.packs.push(PackCall {
                            line: tokens[j].line,
                            format_expression: text(source, &tokens, &args[1]),
                            format,
                            parameters,
                            diagnostics,
                        });
                    }
                    "beacon_inline_execute" if args.len() >= 3 => {
                        inline_arguments.push(
                            args.len() == 3
                                || (args.len() <= 5
                                    && args[3].len() == 1
                                    && tokens[args[3].start].literal.is_none()
                                    && tokens[args[3].start].text == "$null"),
                        );
                        if args.len() == 3 {
                            report.diagnostics.push(format!(
                                "line {}: inline execution omits its argument buffer; no arguments are declared",
                                tokens[j].line
                            ));
                        }
                        if let Some(entry) = constant(&tokens, &args[2], &constants) {
                            command.entrypoints.push(entry);
                        }
                    }
                    "script_resource" | "readbof" => {
                        for arg in &args {
                            command.object_hints.push(text(source, &tokens, arg));
                        }
                    }
                    other
                        if ![
                            "local", "btask", "berror", "blog", "iff", "size", "strlen", "barch",
                            "openf", "readb", "closef",
                        ]
                        .contains(&other) =>
                    {
                        command.calls.push(other.trim_start_matches('&').into())
                    }
                    _ => {}
                }
            }
            command.object_hints.sort();
            command.object_hints.dedup();
            command.entrypoints.sort();
            command.entrypoints.dedup();
            command.calls.sort();
            command.calls.dedup();
            command.no_arguments = !inline_arguments.is_empty()
                && inline_arguments.iter().all(|empty| *empty)
                && command.packs.is_empty();
            report.commands.push(command);
            i = end + 1;
        } else {
            i += 1;
        }
    }
    if source.contains("include(") || source.contains("include (") {
        report
            .diagnostics
            .push("included CNA files are not evaluated; analyze them separately".into());
    }
    report.diagnostics.push("Static metadata only. Values are packed in wire order; CNA defaults, validation, transformations, control flow, and target code-page conversion are not executed.".into());
    Ok(report)
}
