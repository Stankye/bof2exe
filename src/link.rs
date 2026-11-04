//! Deterministic partial COFF linking and declarative local object composition.
//! Produces relocatable COFF, retaining unresolved externals for the runtime/linker.
use crate::coff::{Coff, Relocation, Symbol};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema: u32,
    pub name: String,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub objects: Vec<PathBuf>,
    #[serde(default)]
    pub libraries: Vec<PathBuf>,
    #[serde(default)]
    pub whole_archive: bool,
    #[serde(default)]
    pub resources: Vec<Resource>,
}
fn default_entry() -> String {
    "go".into()
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub path: PathBuf,
    pub symbol: String,
}
impl Spec {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = crate::read_input(path)?;
        let mut spec: Self = toml::from_str(std::str::from_utf8(&bytes)?)?;
        ensure!(
            spec.schema == 1,
            "unsupported link spec schema {}",
            spec.schema
        );
        ensure!(
            !spec.name.trim().is_empty() && !spec.entry.is_empty() && !spec.entry.contains('\0'),
            "invalid spec name/entry"
        );
        let base = path.parent().unwrap_or(Path::new("."));
        for object in &mut spec.objects {
            if object.is_relative() {
                *object = base.join(&object);
            }
        }
        for library in &mut spec.libraries {
            if library.is_relative() {
                *library = base.join(&library);
            }
        }
        for resource in &mut spec.resources {
            if resource.path.is_relative() {
                resource.path = base.join(&resource.path);
            }
        }
        Ok(spec)
    }
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub input_count: usize,
    pub section_count: usize,
    pub resolved: Vec<String>,
    pub unresolved: Vec<String>,
    pub common_symbols: Vec<String>,
    pub weak_aliases: Vec<String>,
    pub resources: Vec<String>,
    pub comdat_discarded: Vec<String>,
    pub notes: Vec<String>,
}
pub struct Linked {
    pub bytes: Vec<u8>,
    pub report: Report,
}
pub struct Input<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}
pub struct ResourceInput<'a> {
    pub symbol: &'a str,
    pub bytes: &'a [u8],
}
#[derive(Clone)]
struct OutputSection {
    name: String,
    size: usize,
    data: Vec<u8>,
    flags: u32,
    relocations: Vec<Relocation>,
}

/// Normalize common and weak-alias symbols when required by an individual object.
pub fn prepare(bytes: &[u8]) -> Result<Cow<'_, [u8]>> {
    let file = Coff::parse(bytes)?;
    if file.sections.iter().any(|s| s.flags & 0x1000 != 0)
        || file
            .symbols
            .iter()
            .flatten()
            .any(|s| s.is_weak() || (s.storage == 2 && s.section == 0 && s.value > 0))
    {
        Ok(Cow::Owned(
            combine(
                &[Input {
                    name: "object",
                    bytes,
                }],
                &[],
            )?
            .bytes,
        ))
    } else {
        Ok(Cow::Borrowed(bytes))
    }
}

/// Combine local objects, resolve strong/common/weak symbols, and append resources.
/// COMDAT groups are selected before symbol resolution. Other duplicate strong
/// definitions are errors; explicit library selection is provided by `library::link`.
pub fn combine(inputs: &[Input<'_>], resources: &[ResourceInput<'_>]) -> Result<Linked> {
    ensure!(
        !inputs.is_empty() && inputs.len() <= 1024,
        "provide between 1 and 1024 objects"
    );
    ensure!(resources.len() <= 1024, "provide at most 1024 resources");
    let resource_size = resources.iter().try_fold(0usize, |n, v| {
        n.checked_add(v.bytes.len())
            .context("resource size overflow")
    })?;
    let total = inputs.iter().try_fold(resource_size, |n, v| {
        n.checked_add(v.bytes.len()).context("input size overflow")
    })?;
    ensure!(total <= crate::MAX_INPUT, "combined inputs exceed 64 MiB");
    let files = inputs
        .iter()
        .map(|i| Coff::parse(i.bytes).with_context(|| format!("parse {}", i.name)))
        .collect::<Result<Vec<_>>>()?;
    let machine = files[0].machine;
    ensure!(
        files.iter().all(|f| f.machine == machine),
        "cannot link different architectures"
    );
    let selection = crate::comdat::select(&files.iter().collect::<Vec<_>>())?;
    let mut section_maps: Vec<Vec<Option<usize>>> =
        files.iter().map(|f| vec![None; f.sections.len()]).collect();
    let mut section_count = 0usize;
    for (fi, mapping) in section_maps.iter_mut().enumerate() {
        for (si, slot) in mapping.iter_mut().enumerate() {
            if selection.keep[fi][si] {
                *slot = Some(section_count);
                section_count += 1;
            }
        }
    }
    ensure!(
        section_count <= i16::MAX as usize,
        "standard COFF output exceeds 32767 sections"
    );
    let mut sections = Vec::new();
    let mut symbols = Vec::<Symbol>::new();
    let mut maps = Vec::new();
    let mut report = Report {
        input_count: inputs.len(),
        section_count: 0,
        resolved: Vec::new(),
        unresolved: Vec::new(),
        common_symbols: Vec::new(),
        weak_aliases: Vec::new(),
        resources: Vec::new(),
        comdat_discarded: selection
            .discarded
            .iter()
            .map(|&(f, s)| format!("{}:{}", inputs[f].name, files[f].sections[s].name))
            .collect(),
        notes: Vec::new(),
    };
    for (fi, file) in files.iter().enumerate() {
        let mut map = vec![None; file.symbols.len()];
        for (index, symbol) in file
            .symbols
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
        {
            let mut symbol = symbol.clone();
            if symbol.section > 0 {
                let source = symbol.section as usize - 1;
                if let Some(target) = section_maps[fi][source] {
                    symbol.section = target as i32 + 1;
                } else if symbol.storage == 2 {
                    // The selected definition resolves references to a discarded external.
                    symbol.section = 0;
                    symbol.value = 0;
                    symbol.auxiliary.clear();
                } else if symbol.storage == 3
                    && symbol.value == 0
                    && symbol.name == file.sections[source].name
                {
                    if let Some((tf, ts)) = selection.replacement[fi][source] {
                        symbol.section = section_maps[tf][ts]
                            .context("COMDAT replacement was discarded")?
                            as i32
                            + 1;
                        symbol.auxiliary.clear();
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            map[index] = Some(symbols.len());
            symbols.push(symbol);
        }
        for (si, section) in file.sections.iter().enumerate() {
            if !selection.keep[fi][si] {
                continue;
            }
            sections.push(OutputSection {
                name: section.name.clone(),
                size: section.size,
                data: section.data.to_vec(),
                flags: section.flags & !0x0100_1000,
                relocations: section.relocations.clone(),
            });
        }
        maps.push(map);
    }
    let mut definitions = BTreeMap::<String, usize>::new();
    for (index, symbol) in symbols.iter().enumerate() {
        if symbol.storage == 2 && (symbol.section > 0 || symbol.section == -1) {
            ensure!(
                definitions.insert(symbol.name.clone(), index).is_none(),
                "duplicate strong definition: {}",
                symbol.name
            );
        }
    }
    for resource in resources {
        ensure!(
            !resource.symbol.is_empty()
                && resource.symbol.len() <= 4096
                && resource
                    .symbol
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "invalid resource symbol"
        );
        ensure!(
            resource.bytes.len() <= crate::MAX_INPUT,
            "resource exceeds 64 MiB"
        );
        let mut data = resource.bytes.to_vec();
        data.resize(data.len().div_ceil(4) * 4, 0);
        let length_offset = data.len() as u32;
        data.extend((resource.bytes.len() as u32).to_le_bytes());
        let section = sections.len() as i32 + 1;
        for (name, value) in [
            (resource.symbol.to_owned(), 0),
            (format!("{}_len", resource.symbol), length_offset),
        ] {
            ensure!(
                !definitions.contains_key(&name),
                "resource symbol collides with definition {name}"
            );
            definitions.insert(name.clone(), symbols.len());
            symbols.push(Symbol {
                name,
                value,
                section,
                storage: 2,
                typ: 0,
                auxiliary: Vec::new(),
            });
        }
        sections.push(OutputSection {
            name: format!(".rdata${}", resource.symbol),
            size: data.len(),
            data,
            flags: 0x40500040,
            relocations: Vec::new(),
        });
        report.resources.push(resource.symbol.into());
    }
    let mut commons = BTreeMap::<String, u32>::new();
    for symbol in &symbols {
        if symbol.storage == 2
            && symbol.section == 0
            && symbol.value > 0
            && !definitions.contains_key(&symbol.name)
        {
            commons
                .entry(symbol.name.clone())
                .and_modify(|size| *size = (*size).max(symbol.value))
                .or_insert(symbol.value);
        }
    }
    if !commons.is_empty() {
        let mut size = 0usize;
        let section = sections.len() as i32 + 1;
        for (name, bytes) in commons {
            size = size.div_ceil(16) * 16;
            let value = u32::try_from(size)?;
            size = size
                .checked_add(bytes as usize)
                .context("common size overflow")?;
            ensure!(size <= crate::MAX_INPUT, "common allocation exceeds 64 MiB");
            definitions.insert(name.clone(), symbols.len());
            report.common_symbols.push(name.clone());
            symbols.push(Symbol {
                name,
                value,
                section,
                storage: 2,
                typ: 0,
                auxiliary: Vec::new(),
            });
        }
        sections.push(OutputSection {
            name: ".bss$common".into(),
            size,
            data: Vec::new(),
            flags: 0xc0500080,
            relocations: Vec::new(),
        });
    }
    let mut redirects: Vec<_> = (0..symbols.len()).collect();
    for (index, symbol) in symbols.iter().enumerate() {
        if [2, 105].contains(&symbol.storage) && symbol.section == 0 {
            if let Some(&target) = definitions.get(&symbol.name) {
                redirects[index] = target;
                report.resolved.push(symbol.name.clone());
            }
        }
    }
    for (file_index, file) in files.iter().enumerate() {
        for (index, symbol) in file
            .symbols
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
        {
            if !symbol.is_weak() {
                continue;
            }
            let current = maps[file_index][index].context("weak symbol missing")?;
            if redirects[current] != current {
                continue;
            }
            let auxiliary = symbol
                .auxiliary
                .first()
                .context("weak symbol has no auxiliary record")?;
            let target = u32::from_le_bytes(auxiliary[..4].try_into()?) as usize;
            let search = u32::from_le_bytes(auxiliary[4..8].try_into()?);
            ensure!(
                [1, 2, 3].contains(&search),
                "weak symbol {} requires unsupported anti-dependency search mode {search}",
                symbol.name
            );
            let target = maps[file_index]
                .get(target)
                .and_then(|v| *v)
                .context("invalid weak fallback symbol")?;
            redirects[current] = target;
            report.weak_aliases.push(symbol.name.clone());
        }
    }
    // Resolve aliases with path compression, including unused cycles, in linear time.
    let mut targets = vec![usize::MAX; redirects.len()];
    let mut visiting = vec![false; redirects.len()];
    for start in 0..redirects.len() {
        if targets[start] != usize::MAX {
            continue;
        }
        let mut path = Vec::new();
        let mut current = start;
        while targets[current] == usize::MAX && redirects[current] != current {
            ensure!(!visiting[current], "cyclic weak symbol aliases");
            visiting[current] = true;
            path.push(current);
            current = redirects[current];
        }
        let target = if targets[current] == usize::MAX {
            current
        } else {
            targets[current]
        };
        targets[current] = target;
        for item in path {
            targets[item] = target;
            visiting[item] = false;
        }
    }
    let mut section_index = 0;
    let mut referenced = BTreeSet::new();
    for (file_index, file) in files.iter().enumerate() {
        for (si, _) in file.sections.iter().enumerate() {
            if !selection.keep[file_index][si] {
                continue;
            }
            for relocation in &mut sections[section_index].relocations {
                if relocation.typ == 0 {
                    relocation.symbol = 0;
                    continue;
                }
                let old = maps[file_index]
                    .get(relocation.symbol)
                    .and_then(|v| *v)
                    .context(
                        "live relocation targets an unmappable local symbol in a discarded COMDAT",
                    )?;
                relocation.symbol = targets[old];
                referenced.insert(relocation.symbol);
            }
            section_index += 1;
        }
    }
    // Remove aliases and auxiliary records, rebuilding all symbol indices.
    let mut compact = Vec::new();
    let mut compact_map = vec![None; symbols.len()];
    for (index, mut symbol) in symbols.into_iter().enumerate() {
        if targets[index] != index || symbol.is_weak() {
            continue;
        }
        if symbol.storage == 2 && symbol.section == 0 && referenced.contains(&index) {
            report.unresolved.push(symbol.name.clone());
        }
        symbol.auxiliary.clear();
        compact_map[index] = Some(compact.len());
        compact.push(symbol);
    }
    for section in &mut sections {
        for relocation in &mut section.relocations {
            if relocation.typ != 0 {
                relocation.symbol = compact_map[relocation.symbol]
                    .context("unresolved weak symbol cannot be emitted")?;
            }
        }
    }
    report.resolved.sort();
    report.resolved.dedup();
    report.unresolved.sort();
    report.unresolved.dedup();
    report.section_count = sections.len();
    let bytes = emit(machine, &sections, &compact)?;
    Ok(Linked { bytes, report })
}

fn emit(machine: u16, sections: &[OutputSection], symbols: &[Symbol]) -> Result<Vec<u8>> {
    ensure!(
        sections.len() <= i16::MAX as usize,
        "standard COFF output exceeds 32767 sections"
    );
    ensure!(
        symbols.len() <= 1_000_000,
        "output exceeds one million symbols"
    );
    let mut strings = vec![0u8; 4];
    let mut names = BTreeMap::<String, u32>::new();
    fn intern(
        value: &str,
        strings: &mut Vec<u8>,
        names: &mut BTreeMap<String, u32>,
    ) -> Result<u32> {
        ensure!(
            !value.contains('\0') && value.len() <= 4096,
            "invalid COFF output name"
        );
        if let Some(&offset) = names.get(value) {
            return Ok(offset);
        }
        ensure!(
            strings.len() + value.len() < crate::MAX_INPUT,
            "COFF string table exceeds limit"
        );
        let offset = strings.len() as u32;
        strings.extend(value.as_bytes());
        strings.push(0);
        names.insert(value.into(), offset);
        Ok(offset)
    }
    let mut out = vec![0u8; 20 + sections.len() * 40];
    fn word(out: &mut [u8], at: usize, v: u16) {
        out[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn dword(out: &mut [u8], at: usize, v: u32) {
        out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    word(&mut out, 0, machine);
    word(&mut out, 2, sections.len() as u16);
    for (i, section) in sections.iter().enumerate() {
        ensure!(
            section.size <= crate::MAX_INPUT && section.data.len() <= section.size,
            "invalid output section size"
        );
        let row = 20 + i * 40;
        let name = if section.name.len() > 8 {
            format!("/{}", intern(&section.name, &mut strings, &mut names)?)
        } else {
            section.name.clone()
        };
        ensure!(name.len() <= 8, "long section name offset is too large");
        out[row..row + name.len()].copy_from_slice(name.as_bytes());
        dword(&mut out, row + 16, section.size as u32);
        let raw = if section.data.is_empty() {
            0
        } else {
            out.len() as u32
        };
        dword(&mut out, row + 20, raw);
        out.extend(&section.data);
        let reloc_offset = if section.relocations.is_empty() {
            0
        } else {
            out.len() as u32
        };
        dword(&mut out, row + 24, reloc_offset);
        let extended = section.relocations.len() > 65535;
        word(
            &mut out,
            row + 32,
            if extended {
                65535
            } else {
                section.relocations.len() as u16
            },
        );
        dword(
            &mut out,
            row + 36,
            section.flags | if extended { 0x0100_0000 } else { 0 },
        );
        if extended {
            out.extend(((section.relocations.len() + 1) as u32).to_le_bytes());
            out.extend([0; 6]);
        }
        for reloc in &section.relocations {
            out.extend((reloc.offset as u32).to_le_bytes());
            out.extend((reloc.symbol as u32).to_le_bytes());
            out.extend(reloc.typ.to_le_bytes());
        }
        ensure!(out.len() <= crate::MAX_INPUT, "COFF output exceeds 64 MiB");
    }
    let symbol_offset = out.len() as u32;
    dword(&mut out, 8, symbol_offset);
    dword(&mut out, 12, symbols.len() as u32);
    for symbol in symbols {
        let mut row = [0u8; 18];
        if symbol.name.len() > 8 {
            dword(&mut row, 4, intern(&symbol.name, &mut strings, &mut names)?);
        } else {
            row[..symbol.name.len()].copy_from_slice(symbol.name.as_bytes());
        }
        dword(&mut row, 8, symbol.value);
        word(&mut row, 12, i16::try_from(symbol.section)? as u16);
        word(&mut row, 14, symbol.typ);
        row[16] = symbol.storage;
        out.extend(row);
    }
    let string_len = strings.len() as u32;
    dword(&mut strings, 0, string_len);
    out.extend(strings);
    ensure!(out.len() <= crate::MAX_INPUT, "COFF output exceeds 64 MiB");
    Coff::parse(&out).context("validate emitted COFF")?;
    Ok(out)
}
