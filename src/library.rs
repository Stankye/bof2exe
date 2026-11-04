//! Bounded in-memory readers for COFF archives and ZIP collections of COFF objects.
//! Members are never extracted to disk. Thin archives and encrypted/ZIP64 inputs
//! require different contracts and are rejected rather than acquiring other files.
use crate::coff::Coff;
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{borrow::Cow, collections::BTreeSet, io::Read};

pub struct Member<'a> {
    pub name: String,
    pub bytes: Cow<'a, [u8]>,
}
pub struct Library<'a> {
    pub format: &'static str,
    pub members: Vec<Member<'a>>,
    pub ignored: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct MemberReport {
    pub ordinal: usize,
    pub name: String,
    pub architecture: String,
    pub size: usize,
    pub definitions: Vec<String>,
    pub references: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub format: String,
    pub members: Vec<MemberReport>,
    pub ignored: Vec<String>,
}

fn span(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    bytes
        .get(at..at.checked_add(len).context("library offset overflow")?)
        .context("truncated library record")
}
fn word(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(span(bytes, at, 2)?.try_into()?))
}
fn dword(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(span(bytes, at, 4)?.try_into()?))
}
fn decimal(bytes: &[u8]) -> Result<usize> {
    let value = std::str::from_utf8(bytes)?.trim();
    ensure!(
        !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()),
        "invalid decimal archive field"
    );
    Ok(value.parse()?)
}

/// Read an ar/COFF `.lib`/`.a` or a ZIP32 collection. ZIP object members may be
/// stored or deflated; their declared lengths and CRC-32 values are checked.
pub fn read(bytes: &[u8]) -> Result<Library<'_>> {
    ensure!(bytes.len() <= crate::MAX_INPUT, "library exceeds 64 MiB");
    ensure!(
        !bytes.starts_with(b"!<thin>\n"),
        "thin archives are unsupported; provide a self-contained library"
    );
    let library = if bytes.starts_with(b"!<arch>\n") {
        read_ar(bytes)?
    } else if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        read_zip(bytes)?
    } else {
        anyhow::bail!("expected a COFF archive (.lib/.a) or ZIP32 object collection");
    };
    let mut names = BTreeSet::new();
    for member in &library.members {
        ensure!(
            member.name.len() <= 4096 && !member.name.contains('\0'),
            "invalid library member name"
        );
        ensure!(
            names.insert(member.name.clone()) || library.format == "coff-archive",
            "duplicate library member name: {}",
            member.name
        );
        Coff::parse(&member.bytes).with_context(|| {
            format!(
                "{} is not a supported COFF object (import-library/LTCG members are unsupported)",
                member.name
            )
        })?;
    }
    Ok(library)
}
impl Library<'_> {
    pub fn report(&self) -> Result<Report> {
        let mut members = Vec::new();
        for (index, member) in self.members.iter().enumerate() {
            let file = Coff::parse(&member.bytes)?;
            let definitions = definitions(&file).into_iter().collect();
            let mut references = BTreeSet::new();
            for section in &file.sections {
                for reloc in &section.relocations {
                    if reloc.typ == 0 {
                        continue;
                    }
                    let symbol = file.symbols[reloc.symbol]
                        .as_ref()
                        .context("missing relocation symbol")?;
                    if symbol.section == 0 && symbol.value == 0 {
                        references.insert(symbol.name.clone());
                    }
                }
            }
            members.push(MemberReport {
                ordinal: index + 1,
                name: member.name.clone(),
                architecture: match file.machine {
                    0x8664 => "x86_64",
                    0x14c => "x86",
                    _ => "aarch64",
                }
                .into(),
                size: member.bytes.len(),
                definitions,
                references: references.into_iter().collect(),
            });
        }
        Ok(Report {
            format: self.format.into(),
            members,
            ignored: self.ignored.clone(),
        })
    }
}
pub(crate) fn definitions(file: &Coff<'_>) -> BTreeSet<String> {
    file.symbols
        .iter()
        .flatten()
        .filter(|s| {
            s.storage == 2 && (s.section > 0 || s.section == -1 || (s.section == 0 && s.value > 0))
        })
        .map(|s| s.name.clone())
        .collect()
}

fn read_ar(bytes: &[u8]) -> Result<Library<'_>> {
    let mut at = 8;
    let mut raw = Vec::new();
    let mut names: Option<&[u8]> = None;
    while at < bytes.len() {
        ensure!(raw.len() < 4096, "archive exceeds 4096 records");
        let header = span(bytes, at, 60)?;
        ensure!(&header[58..] == b"`\n", "invalid archive member header");
        let name = std::str::from_utf8(&header[..16])?.trim_end().to_string();
        let length = decimal(&header[48..58])?;
        let data = span(bytes, at + 60, length)?;
        if name == "//" {
            ensure!(
                names.replace(data).is_none(),
                "duplicate archive longnames table"
            );
        }
        raw.push((name, data));
        at = at
            .checked_add(60 + length)
            .context("archive length overflow")?;
        if at % 2 == 1 {
            ensure!(
                span(bytes, at, 1)? == b"\n",
                "invalid archive alignment padding"
            );
            at += 1;
        }
    }
    let mut result = Library {
        format: "coff-archive",
        members: Vec::new(),
        ignored: Vec::new(),
    };
    for (raw_name, mut data) in raw {
        if raw_name == "/"
            || raw_name == "//"
            || raw_name == "/SYM64/"
            || raw_name.starts_with("__.SYMDEF")
            || raw_name == "/<ECSYMBOLS>/"
        {
            result.ignored.push(raw_name);
            continue;
        }
        let name = if let Some(length) = raw_name.strip_prefix("#1/") {
            let len = decimal(length.as_bytes())?;
            let name = span(data, 0, len)?;
            data = &data[len..];
            std::str::from_utf8(name)?
                .trim_end_matches('\0')
                .to_string()
        } else if let Some(offset) = raw_name.strip_prefix('/') {
            let offset = decimal(offset.as_bytes())?;
            let names = names.context("archive longname has no string table")?;
            ensure!(
                offset == 0
                    || names
                        .get(offset.wrapping_sub(1))
                        .is_some_and(|c| *c == 0 || *c == b'\n'),
                "archive longname offset is not a string boundary"
            );
            let tail = names
                .get(offset..)
                .context("archive longname offset out of bounds")?;
            let end = tail
                .iter()
                .position(|&c| c == 0 || c == b'\n')
                .context("unterminated archive longname")?;
            std::str::from_utf8(&tail[..end])?
                .trim_end_matches('/')
                .to_string()
        } else {
            raw_name.trim_end_matches('/').to_string()
        };
        ensure!(!name.is_empty(), "empty archive member name");
        result.members.push(Member {
            name,
            bytes: Cow::Borrowed(data),
        });
    }
    Ok(result)
}

fn read_zip(bytes: &[u8]) -> Result<Library<'_>> {
    let footer = (bytes.len().saturating_sub(65557)..bytes.len().saturating_sub(21))
        .rev()
        .find(|&at| {
            bytes.get(at..at + 4) == Some(b"PK\x05\x06")
                && word(bytes, at + 20).is_ok_and(|len| at + 22 + len as usize == bytes.len())
        })
        .context("ZIP end-of-central-directory record not found")?;
    ensure!(
        word(bytes, footer + 4)? == 0 && word(bytes, footer + 6)? == 0,
        "multi-disk ZIP libraries are unsupported"
    );
    let count = word(bytes, footer + 10)? as usize;
    ensure!(
        count <= 4096 && word(bytes, footer + 8)? as usize == count,
        "invalid ZIP entry count or unsupported ZIP64"
    );
    let offset = dword(bytes, footer + 16)? as usize;
    let length = dword(bytes, footer + 12)? as usize;
    ensure!(
        offset.checked_add(length) == Some(footer),
        "invalid ZIP central-directory range or unsupported ZIP64"
    );
    let mut at = offset;
    let mut total = 0usize;
    let mut ranges = Vec::new();
    let mut seen = BTreeSet::new();
    let mut result = Library {
        format: "zip-coff",
        members: Vec::new(),
        ignored: Vec::new(),
    };
    for _ in 0..count {
        let row = span(bytes, at, 46)?;
        ensure!(&row[..4] == b"PK\x01\x02", "invalid ZIP directory record");
        let flags = word(row, 8)?;
        let method = word(row, 10)?;
        let crc = dword(row, 16)?;
        let compressed = dword(row, 20)? as usize;
        let size = dword(row, 24)? as usize;
        ensure!(
            flags & !0x080e == 0,
            "encrypted or unsupported ZIP entry flags"
        );
        ensure!(
            method == 0 || method == 8,
            "ZIP compression method {method} is unsupported"
        );
        ensure!(
            word(row, 34)? == 0,
            "multi-disk ZIP entries are unsupported"
        );
        let name_len = word(row, 28)? as usize;
        let extra_len = word(row, 30)? as usize;
        let comment_len = word(row, 32)? as usize;
        let name = std::str::from_utf8(span(bytes, at + 46, name_len)?)?.to_string();
        ensure!(
            !name.is_empty() && name.len() <= 4096 && seen.insert(name.clone()),
            "empty, oversized, or duplicate ZIP member name"
        );
        at = at
            .checked_add(46 + name_len + extra_len + comment_len)
            .context("ZIP directory size overflow")?;
        ensure!(at <= footer, "ZIP directory record exceeds its range");
        total = total
            .checked_add(size)
            .context("ZIP decoded size overflow")?;
        ensure!(
            total <= crate::MAX_INPUT,
            "ZIP decoded contents exceed 64 MiB"
        );
        let local = dword(row, 42)? as usize;
        let header = span(bytes, local, 30)?;
        ensure!(
            &header[..4] == b"PK\x03\x04"
                && word(header, 6)? == flags
                && word(header, 8)? == method,
            "ZIP local and central headers disagree"
        );
        let local_name = word(header, 26)? as usize;
        let local_extra = word(header, 28)? as usize;
        ensure!(
            span(bytes, local + 30, local_name)? == name.as_bytes(),
            "ZIP member names disagree"
        );
        if flags & 8 == 0 {
            ensure!(
                dword(header, 14)? == crc
                    && dword(header, 18)? as usize == compressed
                    && dword(header, 22)? as usize == size,
                "ZIP size/CRC metadata disagree"
            );
        }
        let start = local
            .checked_add(30 + local_name + local_extra)
            .context("ZIP member offset overflow")?;
        let end = start
            .checked_add(compressed)
            .context("ZIP member length overflow")?;
        ensure!(end <= offset, "ZIP payload overlaps central directory");
        ranges.push(local..end);
        let data = span(bytes, start, compressed)?;
        let extension = name.rsplit('.').next().unwrap_or_default();
        if !(extension.eq_ignore_ascii_case("o") || extension.eq_ignore_ascii_case("obj")) {
            result.ignored.push(name);
            continue;
        }
        let data = if method == 0 {
            ensure!(size == compressed, "stored ZIP member length mismatch");
            data.to_vec()
        } else {
            let mut decoder = flate2::read::DeflateDecoder::new(data);
            let mut output = Vec::new();
            (&mut decoder)
                .take(size as u64 + 1)
                .read_to_end(&mut output)?;
            ensure!(
                decoder.total_in() == compressed as u64,
                "ZIP deflate stream has trailing or truncated bytes"
            );
            output
        };
        ensure!(data.len() == size, "ZIP decoded member length mismatch");
        ensure!(crc32fast::hash(&data) == crc, "ZIP CRC mismatch for {name}");
        result.members.push(Member {
            name,
            bytes: Cow::Owned(data),
        });
    }
    ensure!(at == footer, "ZIP directory count does not match its size");
    ranges.sort_by_key(|r| r.start);
    ensure!(
        ranges.windows(2).all(|p| p[0].end <= p[1].start),
        "overlapping ZIP member records"
    );
    Ok(result)
}

#[derive(Debug, Serialize)]
pub struct SelectionReport {
    pub selected: Vec<String>,
    pub unused: Vec<String>,
}

/// Resolve library members on demand, revisiting earlier libraries for transitive
/// references. `whole` explicitly includes every member of every supplied library.
pub fn link(
    objects: &[crate::link::Input<'_>],
    libraries: &[crate::link::Input<'_>],
    resources: &[crate::link::ResourceInput<'_>],
    whole: bool,
) -> Result<(crate::link::Linked, SelectionReport)> {
    ensure!(
        !objects.is_empty(),
        "library linking needs at least one root object"
    );
    ensure!(libraries.len() <= 1024, "too many libraries");
    ensure!(objects.len() <= 1024, "too many root objects");
    let input_size = objects.iter().chain(libraries).try_fold(0usize, |n, i| {
        n.checked_add(i.bytes.len())
            .context("library input size overflow")
    })?;
    ensure!(
        input_size <= crate::MAX_INPUT,
        "combined root/library files exceed 64 MiB"
    );
    let mut decoded = 0usize;
    let libs = libraries
        .iter()
        .map(|input| {
            let library =
                read(input.bytes).with_context(|| format!("read library {}", input.name))?;
            for member in &library.members {
                decoded = decoded
                    .checked_add(member.bytes.len())
                    .context("library size overflow")?;
            }
            ensure!(
                decoded <= crate::MAX_INPUT,
                "combined library contents exceed 64 MiB"
            );
            Ok(library)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut labels = Vec::new();
    let mut members = Vec::new();
    for (index, library) in libs.iter().enumerate() {
        let mut counts = std::collections::BTreeMap::new();
        for member in &library.members {
            *counts.entry(&member.name).or_insert(0usize) += 1;
        }
        for (ordinal, member) in library.members.iter().enumerate() {
            labels.push(if counts[&member.name] > 1 {
                format!("{}({}:{})", libraries[index].name, ordinal + 1, member.name)
            } else {
                format!("{}({})", libraries[index].name, member.name)
            });
            members.push(member);
        }
    }
    ensure!(
        members.len() <= 4096,
        "combined libraries exceed 4096 members"
    );
    let files = members
        .iter()
        .map(|m| Coff::parse(&m.bytes))
        .collect::<Result<Vec<_>>>()?;
    let roots = objects
        .iter()
        .map(|o| Coff::parse(o.bytes))
        .collect::<Result<Vec<_>>>()?;
    let machine = roots[0].machine;
    ensure!(
        roots.iter().all(|f| f.machine == machine),
        "cannot link different root architectures"
    );
    ensure!(
        !whole || files.iter().all(|f| f.machine == machine),
        "whole-archive includes a different architecture"
    );
    let exports: Vec<_> = files.iter().map(definitions).collect();
    let available: BTreeSet<String> = exports
        .iter()
        .zip(&files)
        .filter(|(_, f)| f.machine == machine)
        .flat_map(|(d, _)| d.iter().cloned())
        .collect();
    let mut selected = vec![whole; members.len()];
    loop {
        let active: Vec<_> = roots
            .iter()
            .chain(
                files
                    .iter()
                    .zip(&selected)
                    .filter_map(|(f, &s)| s.then_some(f)),
            )
            .collect();
        let selected_sections = crate::comdat::select(&active)?;
        let mut defined = BTreeSet::new();
        for (fi, file) in active.iter().enumerate() {
            for symbol in file.symbols.iter().flatten().filter(|s| s.storage == 2) {
                if symbol.section > 0 {
                    if selected_sections.keep[fi][symbol.section as usize - 1] {
                        defined.insert(symbol.name.clone());
                    }
                } else if symbol.section == -1 || symbol.value > 0 {
                    defined.insert(symbol.name.clone());
                }
            }
        }
        for resource in resources {
            defined.insert(resource.symbol.into());
            defined.insert(format!("{}_len", resource.symbol));
        }
        let mut needed = BTreeSet::new();
        for (fi, file) in active.iter().enumerate() {
            for (si, section) in file.sections.iter().enumerate() {
                if !selected_sections.keep[fi][si] {
                    continue;
                }
                for relocation in &section.relocations {
                    if relocation.typ == 0 {
                        continue;
                    }
                    let mut current = relocation.symbol;
                    let mut visited = BTreeSet::new();
                    loop {
                        ensure!(
                            visited.insert(current),
                            "cyclic weak aliases during library selection"
                        );
                        let symbol = file
                            .symbols
                            .get(current)
                            .and_then(Option::as_ref)
                            .context("invalid library reference")?;
                        if symbol.section != 0
                            || symbol.value != 0
                            || defined.contains(&symbol.name)
                        {
                            break;
                        }
                        if symbol.is_weak() {
                            let aux = symbol
                                .auxiliary
                                .first()
                                .context("weak reference lacks auxiliary record")?;
                            let mode = u32::from_le_bytes(aux[4..8].try_into()?);
                            if mode == 2 && available.contains(&symbol.name) {
                                needed.insert(symbol.name.clone());
                                break;
                            }
                            ensure!(
                                [1, 2, 3].contains(&mode),
                                "unsupported weak search mode {mode}"
                            );
                            current = u32::from_le_bytes(aux[..4].try_into()?) as usize;
                        } else {
                            needed.insert(symbol.name.clone());
                            break;
                        }
                    }
                }
            }
        }
        let next = files
            .iter()
            .enumerate()
            .find(|(i, f)| {
                !selected[*i]
                    && f.machine == machine
                    && exports[*i].iter().any(|name| needed.contains(name))
            })
            .map(|(i, _)| i);
        if let Some(index) = next {
            selected[index] = true;
        } else {
            break;
        }
    }
    let mut inputs: Vec<_> = objects
        .iter()
        .map(|i| crate::link::Input {
            name: i.name,
            bytes: i.bytes,
        })
        .collect();
    let mut report = SelectionReport {
        selected: Vec::new(),
        unused: Vec::new(),
    };
    for (index, member) in members.iter().enumerate() {
        if selected[index] {
            report.selected.push(labels[index].clone());
            inputs.push(crate::link::Input {
                name: &labels[index],
                bytes: &member.bytes,
            });
        } else {
            report.unused.push(labels[index].clone());
        }
    }
    let mut linked = crate::link::combine(&inputs, resources)?;
    linked.report.notes.push(format!(
        "selected {} library members; left {} unused",
        report.selected.len(),
        report.unused.len()
    ));
    Ok((linked, report))
}
