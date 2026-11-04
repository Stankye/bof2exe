//! Cross-platform static compatibility checks. DLL availability is checked at load time.
use crate::coff::Coff;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Implemented Beacon API subset.
pub const BEACON_APIS: &[&str] = &[
    "BeaconDataParse",
    "BeaconDataInt",
    "BeaconDataShort",
    "BeaconDataLength",
    "BeaconDataExtract",
    "BeaconDataPtr",
    "BeaconOutput",
    "BeaconPrintf",
    "BeaconFormatAlloc",
    "BeaconFormatReset",
    "BeaconFormatFree",
    "BeaconFormatAppend",
    "BeaconFormatPrintf",
    "BeaconFormatToString",
    "BeaconFormatInt",
    "toWideChar",
];
/// Ordinary CRT memory/string helpers used by compiler output and local modules.
pub const CRT_APIS: &[&str] = &[
    "memcpy",
    "memmove",
    "memset",
    "memcmp",
    "strlen",
    "strcmp",
    "strncmp",
    "strcpy",
    "strncpy",
    "strcat",
    "strncat",
    "_stricmp",
    "_strnicmp",
    "wcslen",
    "wcscmp",
    "wcsncmp",
    "_wcsicmp",
];
pub const COMPILER_APIS: &[&str] = &["__chkstk", "___chkstk_ms", "__chkstk_ms"];
/// Unqualified Windows imports conventionally supplied by BOF hosts.
pub const LOADER_APIS: &[&str] = &[
    "LoadLibraryA",
    "GetModuleHandleA",
    "GetProcAddress",
    "FreeLibrary",
];
#[derive(Debug, Clone, Serialize)]
pub struct SectionInfo {
    pub name: String,
    pub size: usize,
    pub relocations: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub format: String,
    pub normalized: bool,
    pub architecture: String,
    pub sha256: String,
    pub sections: Vec<SectionInfo>,
    pub defined_symbols: Vec<String>,
    pub imports: Vec<String>,
    pub issues: Vec<String>,
    pub notes: Vec<String>,
}
impl Report {
    pub fn compatible(&self) -> bool {
        self.issues.is_empty()
    }
}
pub fn import_name(name: &str) -> &str {
    name.strip_prefix("__imp_").unwrap_or(name)
}

pub fn inspect(bytes: &[u8], entry: &str) -> anyhow::Result<Report> {
    let prepared = crate::link::prepare(bytes)?;
    let mut report = inspect_prepared(&prepared, entry)?;
    report.sha256 = format!("{:x}", Sha256::digest(bytes));
    if matches!(prepared, std::borrow::Cow::Owned(_)) {
        report.normalized = true;
        if bytes.starts_with(&[0, 0, 0xff, 0xff]) {
            report.format = "bigobj".into();
        }
        report.notes.push("common/weak symbols normalized before loading; section report describes the normalized layout".into());
    }
    Ok(report)
}
fn inspect_prepared(bytes: &[u8], entry: &str) -> anyhow::Result<Report> {
    let file = Coff::parse(bytes)?;
    let mut report = Report {
        format: if file.bigobj { "bigobj" } else { "coff" }.into(),
        normalized: false,
        architecture: match file.machine {
            0x8664 => "x86_64",
            0x14c => "x86",
            _ => "aarch64",
        }
        .into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        sections: Vec::new(),
        defined_symbols: Vec::new(),
        imports: Vec::new(),
        issues: Vec::new(),
        notes: Vec::new(),
    };
    if file.machine != 0x8664 {
        report
            .issues
            .push("execution supports Windows x64 objects only".into());
    }
    let mut total = 0usize;
    let mut referenced = BTreeSet::new();
    for section in &file.sections {
        let name = &section.name;
        total = total.saturating_add(section.size.max(1).div_ceil(4096) * 4096);
        if (section.flags >> 20) & 15 > 13 {
            report
                .issues
                .push(format!("section {name} requires unsupported alignment"));
        }
        if section.writable() && section.executable() {
            report.issues.push(format!(
                "section {name} requires writable executable memory"
            ));
        }
        for reloc in &section.relocations {
            let width = match reloc.typ {
                0 => continue,
                1 => 8,
                2..=9 | 11 => 4,
                10 => 2,
                typ => {
                    report.issues.push(format!(
                        "section {name}: unsupported relocation 0x{typ:04x}"
                    ));
                    0
                }
            };
            referenced.insert(reloc.symbol);
            if reloc
                .offset
                .checked_add(width)
                .is_none_or(|end| end > section.size)
            {
                report.issues.push(format!(
                    "section {name}: relocation at {} is out of bounds",
                    reloc.offset
                ));
            }
        }
        if name == ".drectve"
            && section
                .data
                .iter()
                .any(|b| !b.is_ascii_whitespace() && *b != 0)
        {
            report
                .issues
                .push("linker directives (.drectve) are unsupported".into());
        }
        if name.starts_with(".tls") {
            report
                .issues
                .push("TLS initialization is unsupported".into());
        }
        if name.starts_with(".CRT") || name.starts_with(".ctors") {
            report
                .issues
                .push("static constructors are unsupported".into());
        }
        if (name == ".pdata" || name.starts_with(".pdata$"))
            && !section.data.is_empty()
            && section.size % 12 != 0
        {
            report
                .issues
                .push("AMD64 .pdata size must be a multiple of 12".into());
        }
        report.sections.push(SectionInfo {
            name: name.clone(),
            size: section.size,
            relocations: section.relocations.len(),
        });
    }
    if total > crate::MAX_INPUT {
        report.issues.push("mapped sections exceed 64 MiB".into());
    }
    let mut found_entry = false;
    let mut imports = BTreeSet::new();
    for (index, symbol) in file
        .symbols
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
    {
        let name = &symbol.name;
        if symbol.section == 0 && referenced.contains(&index) {
            if symbol.value != 0 {
                report
                    .issues
                    .push(format!("common symbol {name} requires -fno-common"));
                continue;
            }
            imports.insert(name.clone());
            let normalized = import_name(name);
            if !BEACON_APIS.contains(&normalized)
                && !CRT_APIS.contains(&normalized)
                && !COMPILER_APIS.contains(&normalized)
                && !LOADER_APIS.contains(&normalized)
                && !valid_dll_import(normalized)
            {
                report
                    .issues
                    .push(format!("unsupported external symbol: {name}"));
            }
        } else if symbol.section > 0 {
            let section = &file.sections[symbol.section as usize - 1];
            if name == entry {
                found_entry = section.executable() && (symbol.value as usize) < section.size;
            }
            if !name.is_empty() {
                report.defined_symbols.push(name.clone());
            }
        }
    }
    if !found_entry {
        report.issues.push(format!(
            "entrypoint {entry:?} was not found in a code section"
        ));
    }
    report.imports = imports.into_iter().collect();
    report.defined_symbols.sort();
    report.defined_symbols.dedup();
    report.issues.sort();
    report.issues.dedup();
    report.notes.sort();
    report.notes.dedup();
    Ok(report)
}
pub(crate) fn valid_dll_import(name: &str) -> bool {
    let Some((dll, export)) = name.split_once('$') else {
        return false;
    };
    !dll.is_empty()
        && dll
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        && dll != "."
        && dll != ".."
        && !export.is_empty()
        && !export.contains(['\0', '$'])
}
