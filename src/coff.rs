//! Bounds-checked reader for standard and BigObj Microsoft COFF objects.
//! Offsets and constants follow Microsoft's PE/COFF specification.
use anyhow::{ensure, Context, Result};

pub const BIGOBJ_CLASS_ID: [u8; 16] = [
    0xc7, 0xa1, 0xba, 0xd1, 0xee, 0xba, 0xa9, 0x4b, 0xaf, 0x20, 0xfa, 0xf6, 0x6a, 0xa4, 0xdc, 0xb8,
];

#[derive(Debug)]
pub struct Coff<'a> {
    pub machine: u16,
    pub timestamp: u32,
    pub bigobj: bool,
    pub sections: Vec<Section<'a>>,
    /// Raw symbol indices, including vacant auxiliary records.
    pub symbols: Vec<Option<Symbol>>,
}
#[derive(Debug)]
pub struct Section<'a> {
    pub name: String,
    pub size: usize,
    pub data: &'a [u8],
    pub flags: u32,
    pub relocations: Vec<Relocation>,
}
impl Section<'_> {
    pub fn executable(&self) -> bool {
        self.flags & 0x2000_0000 != 0
    }
    pub fn writable(&self) -> bool {
        self.flags & 0x8000_0000 != 0
    }
}
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub value: u32,
    pub section: i32,
    pub storage: u8,
    pub typ: u16,
    pub auxiliary: Vec<Vec<u8>>,
}
#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: usize,
    pub symbol: usize,
    pub typ: u16,
}
impl Symbol {
    pub fn is_weak(&self) -> bool {
        self.storage == 105
            || (self.storage == 2
                && self.section == 0
                && self.value == 0
                && !self.auxiliary.is_empty())
    }
}

fn span(bytes: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    bytes
        .get(offset..offset.checked_add(len).context("COFF offset overflow")?)
        .context("truncated COFF data")
}
fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(span(bytes, offset, 2)?.try_into()?))
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(span(bytes, offset, 4)?.try_into()?))
}
fn string_at(bytes: &[u8], offset: usize) -> Result<String> {
    ensure!(offset >= 4, "invalid COFF string offset");
    let tail = bytes
        .get(offset..)
        .context("COFF string offset out of bounds")?;
    let end = tail
        .iter()
        .position(|&v| v == 0)
        .context("unterminated COFF name")?;
    ensure!(end <= 4096, "COFF name exceeds 4096 bytes");
    Ok(std::str::from_utf8(&tail[..end])
        .context("non-UTF-8 COFF name")?
        .into())
}
fn inline_name(bytes: &[u8]) -> Result<String> {
    let len = bytes.iter().position(|&v| v == 0).unwrap_or(bytes.len());
    Ok(std::str::from_utf8(&bytes[..len])?.into())
}
impl<'a> Coff<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(bytes.len() <= crate::MAX_INPUT, "object exceeds 64 MiB");
        span(bytes, 0, 20)?;
        let bigobj = bytes.starts_with(&[0, 0, 0xff, 0xff]);
        let (header_size, symbol_size, machine, section_count, symbol_offset, symbol_count) =
            if bigobj {
                span(bytes, 0, 56)?;
                ensure!(
                    u16_at(bytes, 4)? >= 2 && bytes[12..28] == BIGOBJ_CLASS_ID,
                    "invalid BigObj header (import libraries and LTCG are unsupported)"
                );
                (
                    56,
                    20,
                    u16_at(bytes, 6)?,
                    u32_at(bytes, 44)? as usize,
                    u32_at(bytes, 48)? as usize,
                    u32_at(bytes, 52)? as usize,
                )
            } else {
                ensure!(
                    u16_at(bytes, 16)? == 0 && u16_at(bytes, 18)? & 2 == 0,
                    "expected a relocatable object, not a PE image"
                );
                (
                    20,
                    18,
                    u16_at(bytes, 0)?,
                    u16_at(bytes, 2)? as usize,
                    u32_at(bytes, 8)? as usize,
                    u32_at(bytes, 12)? as usize,
                )
            };
        ensure!(
            [0x8664, 0x14c, 0xaa64].contains(&machine),
            "expected a COFF object (PE, LTCG, and archives are unsupported)"
        );
        ensure!(
            section_count > 0 && section_count <= 65535,
            "invalid section count"
        );
        span(bytes, header_size, section_count * 40)?;
        ensure!(symbol_count <= 1_000_000, "too many COFF symbols");
        if symbol_count != 0 {
            ensure!(
                symbol_offset >= header_size + section_count * 40,
                "symbol table overlaps headers"
            );
        }
        let symbol_bytes = span(bytes, symbol_offset, symbol_count * symbol_size)?;
        let strings = if symbol_offset != 0 {
            let offset = symbol_offset + symbol_bytes.len();
            let len = u32_at(bytes, offset)? as usize;
            ensure!(len >= 4, "invalid string table length");
            span(bytes, offset, len)?
        } else {
            &[]
        };
        let mut symbols = Vec::with_capacity(symbol_count);
        let mut i = 0;
        let mut name_bytes = 0usize;
        while i < symbol_count {
            let row = span(symbol_bytes, i * symbol_size, symbol_size)?;
            let name = if row[..4] == [0, 0, 0, 0] {
                string_at(strings, u32_at(row, 4)? as usize)?
            } else {
                inline_name(&row[..8])?
            };
            name_bytes += name.len();
            ensure!(name_bytes <= crate::MAX_INPUT, "symbol names exceed 64 MiB");
            let aux = row[symbol_size - 1] as usize;
            ensure!(
                i + aux < symbol_count,
                "auxiliary symbol records exceed table"
            );
            symbols.push(Some(Symbol {
                name,
                value: u32_at(row, 8)?,
                section: if bigobj {
                    u32_at(row, 12)? as i32
                } else {
                    u16_at(row, 12)? as i16 as i32
                },
                storage: row[symbol_size - 2],
                typ: u16_at(row, symbol_size - 4)?,
                auxiliary: (1..=aux)
                    .map(|n| {
                        symbol_bytes[(i + n) * symbol_size..(i + n + 1) * symbol_size].to_vec()
                    })
                    .collect(),
            }));
            symbols.extend((0..aux).map(|_| None));
            i += 1 + aux;
        }
        let mut sections = Vec::with_capacity(section_count);
        for i in 0..section_count {
            let row = span(bytes, header_size + i * 40, 40)?;
            let raw_name = inline_name(&row[..8])?;
            let name = if let Some(offset) = raw_name.strip_prefix('/') {
                string_at(
                    strings,
                    offset.parse().context("invalid long section name offset")?,
                )?
            } else {
                raw_name
            };
            name_bytes += name.len();
            ensure!(name_bytes <= crate::MAX_INPUT, "COFF names exceed 64 MiB");
            let size = u32_at(row, 16)? as usize;
            ensure!(size <= crate::MAX_INPUT, "section exceeds 64 MiB");
            let raw = u32_at(row, 20)? as usize;
            let flags = u32_at(row, 36)?;
            let data = if raw == 0 {
                ensure!(
                    size == 0 || flags & 0x80 != 0,
                    "initialized section has no raw data"
                );
                &[]
            } else {
                span(bytes, raw, size)?
            };
            let mut reloc_count = u16_at(row, 32)? as usize;
            let reloc_offset = u32_at(row, 24)? as usize;
            let extended = flags & 0x0100_0000 != 0;
            if extended {
                ensure!(
                    reloc_count == 65535,
                    "invalid extended relocation count marker"
                );
                reloc_count = u32_at(bytes, reloc_offset)? as usize;
                ensure!(
                    reloc_count > 65535 && reloc_count <= 1_000_001,
                    "invalid extended relocation count"
                );
                ensure!(
                    u32_at(bytes, reloc_offset + 4)? == 0 && u16_at(bytes, reloc_offset + 8)? == 0,
                    "invalid extended relocation sentinel"
                );
            }
            let table = span(bytes, reloc_offset, reloc_count * 10)?;
            let table = if extended { &table[10..] } else { table };
            let mut relocations = Vec::with_capacity(reloc_count);
            for row in table.as_chunks::<10>().0 {
                let symbol = u32_at(row, 4)? as usize;
                let typ = u16_at(row, 8)?;
                if typ != 0 {
                    ensure!(
                        symbols.get(symbol).is_some_and(Option::is_some),
                        "relocation refers to invalid/auxiliary symbol {symbol}"
                    );
                }
                relocations.push(Relocation {
                    offset: u32_at(row, 0)? as usize,
                    symbol,
                    typ,
                });
            }
            sections.push(Section {
                name,
                size,
                data,
                flags,
                relocations,
            });
        }
        for symbol in symbols.iter().flatten() {
            if symbol.section > 0 {
                let section = sections
                    .get(symbol.section as usize - 1)
                    .context("symbol section is out of bounds")?;
                ensure!(
                    symbol.value as usize <= section.size,
                    "symbol lies outside its section"
                );
            } else {
                ensure!(symbol.section >= -2, "invalid symbol section");
            }
        }
        Ok(Self {
            machine,
            timestamp: u32_at(bytes, if bigobj { 8 } else { 4 })?,
            bigobj,
            sections,
            symbols,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Comdat {
    pub selection: u8,
    pub parent: Option<usize>,
    pub key: Option<String>,
}
impl Coff<'_> {
    /// Decode the section-definition auxiliary record for each COMDAT section.
    pub fn comdats(&self) -> Result<Vec<Option<Comdat>>> {
        let mut section_definitions: Vec<Vec<&Symbol>> = vec![Vec::new(); self.sections.len()];
        let mut keys = vec![None; self.sections.len()];
        for symbol in self
            .symbols
            .iter()
            .flatten()
            .filter(|s| s.section > 0 && s.value == 0)
        {
            let index = symbol.section as usize - 1;
            if symbol.storage == 3
                && symbol.typ == 0
                && symbol.name == self.sections[index].name
                && !symbol.auxiliary.is_empty()
            {
                section_definitions[index].push(symbol);
            }
            if symbol.storage == 2 && keys[index].is_none() {
                keys[index] = Some(symbol.name.clone());
            }
        }
        let mut result = Vec::with_capacity(self.sections.len());
        for (index, section) in self.sections.iter().enumerate() {
            if section.flags & 0x1000 == 0 {
                result.push(None);
                continue;
            }
            let definitions = &section_definitions[index];
            ensure!(
                definitions.len() == 1,
                "COMDAT section {} needs one section-definition record",
                section.name
            );
            let aux = &definitions[0].auxiliary[0];
            let selection = aux[14];
            ensure!(
                (1..=7).contains(&selection),
                "invalid COMDAT selection {selection}"
            );
            let parent = if selection == 5 {
                let number = u16::from_le_bytes(aux[12..14].try_into()?) as u32
                    | ((u16::from_le_bytes(aux[16..18].try_into()?) as u32) << 16);
                ensure!(
                    number > 0 && number as usize <= self.sections.len(),
                    "COMDAT associative parent out of bounds"
                );
                Some(number as usize - 1)
            } else {
                None
            };
            let key = keys[index].clone();
            result.push(Some(Comdat {
                selection,
                parent,
                key,
            }));
        }
        Ok(result)
    }
}
