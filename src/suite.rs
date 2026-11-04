//! Declarative local compatibility fixtures and output expectations.
use crate::{args::Argument, runtime::Output};
use anyhow::{ensure, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub schema: u32,
    pub name: String,
    pub cases: Vec<Case>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub object: PathBuf,
    #[serde(default = "entry")]
    pub entry: String,
    #[serde(default)]
    pub arguments: Vec<Argument>,
    #[serde(default = "timeout")]
    pub timeout: u64,
    #[serde(default = "compatible")]
    pub compatible: bool,
    #[serde(default)]
    pub issue_contains: Option<String>,
    #[serde(default)]
    pub expect: Vec<Expectation>,
}
fn entry() -> String {
    "go".into()
}
fn timeout() -> u64 {
    30
}
fn compatible() -> bool {
    true
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    #[serde(default)]
    pub kind: Option<i32>,
    #[serde(default)]
    pub contains: Option<String>,
    #[serde(default)]
    pub exact_base64: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub name: String,
    pub passed: bool,
    pub executed: bool,
    pub sha256: Option<String>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub name: String,
    pub passed: usize,
    pub failed: usize,
    pub mode: String,
    pub cases: Vec<CaseResult>,
}
impl Suite {
    pub fn load(path: &Path) -> Result<Self> {
        let data = crate::read_input(path)?;
        let mut suite: Self = toml::from_str(std::str::from_utf8(&data)?)?;
        ensure!(
            suite.schema == 1 && !suite.name.trim().is_empty(),
            "invalid suite schema/name"
        );
        ensure!(
            !suite.cases.is_empty() && suite.cases.len() <= 1024,
            "suite must contain 1..1024 cases"
        );
        let mut names = BTreeSet::new();
        let base = path.parent().unwrap_or(Path::new("."));
        for case in &mut suite.cases {
            ensure!(
                !case.name.trim().is_empty() && names.insert(case.name.clone()),
                "suite case names must be nonempty and unique"
            );
            ensure!((1..=86400).contains(&case.timeout), "invalid suite timeout");
            ensure!(
                !case.entry.is_empty() && !case.entry.contains('\0'),
                "invalid suite entrypoint"
            );
            ensure!(
                case.compatible || case.expect.is_empty(),
                "incompatible case cannot require output"
            );
            ensure!(
                case.expect.len() <= 64,
                "case exceeds 64 output expectations"
            );
            for expected in &case.expect {
                ensure!(
                    expected.contains.is_some() || expected.exact_base64.is_some(),
                    "output expectation needs contains or exact_base64"
                );
                if let Some(value) = &expected.exact_base64 {
                    STANDARD.decode(value)?;
                }
            }
            if case.object.is_relative() {
                case.object = base.join(&case.object);
            }
        }
        Ok(suite)
    }
}
impl Case {
    /// Verify actual callback bytes, preserving callback order and optional kind filtering.
    pub fn verify(&self, output: &[Output]) -> Result<()> {
        for expected in &self.expect {
            let bytes: Vec<u8> = output
                .iter()
                .filter(|o| expected.kind.is_none_or(|k| k == o.kind))
                .flat_map(|o| o.data.iter().copied())
                .collect();
            if let Some(text) = &expected.contains {
                ensure!(
                    text.is_empty() || bytes.windows(text.len()).any(|v| v == text.as_bytes()),
                    "expected output substring {text:?} was absent (kind {:?})",
                    expected.kind
                );
            }
            if let Some(value) = &expected.exact_base64 {
                ensure!(
                    bytes == STANDARD.decode(value)?,
                    "exact output mismatch (kind {:?})",
                    expected.kind
                );
            }
        }
        Ok(())
    }
}

/// Inspect each case and optionally use a caller-supplied isolated executor.
pub fn run(
    suite: &Suite,
    execute: bool,
    mut executor: impl FnMut(&[u8], &str, &mut [u8], u64) -> Result<Vec<Output>>,
) -> Report {
    let mut report = Report {
        name: suite.name.clone(),
        passed: 0,
        failed: 0,
        mode: if execute { "execute" } else { "inspect" }.into(),
        cases: Vec::new(),
    };
    for case in &suite.cases {
        let mut result = CaseResult {
            name: case.name.clone(),
            passed: false,
            executed: false,
            sha256: None,
            diagnostics: Vec::new(),
        };
        let checked = (|| -> Result<()> {
            let bytes = crate::read_input(&case.object)?;
            let inspection = crate::inspect::inspect(&bytes, &case.entry)?;
            result.sha256 = Some(inspection.sha256.clone());
            ensure!(
                inspection.compatible() == case.compatible,
                "expected compatible={}, issues: {}",
                case.compatible,
                inspection.issues.join("; ")
            );
            if let Some(needle) = &case.issue_contains {
                ensure!(
                    inspection.issues.iter().any(|v| v.contains(needle)),
                    "expected compatibility issue {needle:?} was absent"
                );
            }
            if !case.compatible {
                return Ok(());
            }
            if execute {
                let mut arguments = crate::args::pack(&case.arguments)?;
                result.executed = true;
                let output = executor(&bytes, &case.entry, &mut arguments, case.timeout)?;
                case.verify(&output)?;
            } else if !case.expect.is_empty() {
                result
                    .diagnostics
                    .push("output expectations deferred; use --execute to evaluate them".into());
            }
            Ok(())
        })();
        match checked {
            Ok(()) => {
                result.passed = true;
                report.passed += 1;
            }
            Err(error) => {
                result.diagnostics.push(format!("{error:#}"));
                report.failed += 1;
            }
        }
        report.cases.push(result);
    }
    report
}
