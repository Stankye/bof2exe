#![allow(dead_code)]
//! Small independently constructed records for testing the public COFF reader/linker.
pub struct Section<'a> {
    pub name: &'a str,
    pub data: Vec<u8>,
    pub size: u32,
    pub flags: u32,
    pub relocations: Vec<(u32, u32, u16)>,
}
impl<'a> Section<'a> {
    pub fn text(data: Vec<u8>) -> Self {
        Self {
            name: ".text",
            size: data.len() as u32,
            data,
            flags: 0x60500020,
            relocations: Vec::new(),
        }
    }
}
pub struct Symbol<'a> {
    pub name: &'a str,
    pub value: u32,
    pub section: i32,
    pub storage: u8,
    pub auxiliary: Vec<Vec<u8>>,
}
impl<'a> Symbol<'a> {
    pub fn external(name: &'a str, section: i32, value: u32) -> Self {
        Self {
            name,
            section,
            value,
            storage: 2,
            auxiliary: Vec::new(),
        }
    }
}
pub fn object(
    machine: u16,
    big: bool,
    sections: &[Section<'_>],
    symbols: &[Symbol<'_>],
) -> Vec<u8> {
    fn w(bytes: &mut [u8], at: usize, n: u16) {
        bytes[at..at + 2].copy_from_slice(&n.to_le_bytes());
    }
    fn d(bytes: &mut [u8], at: usize, n: u32) {
        bytes[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    let header = if big { 56 } else { 20 };
    let record = if big { 20 } else { 18 };
    let mut bytes = vec![0u8; header + sections.len() * 40];
    let mut strings = vec![0u8; 4];
    if big {
        w(&mut bytes, 2, 65535);
        w(&mut bytes, 4, 2);
        w(&mut bytes, 6, machine);
        bytes[12..28].copy_from_slice(&bof2exe::coff::BIGOBJ_CLASS_ID);
        d(&mut bytes, 44, sections.len() as u32);
    } else {
        w(&mut bytes, 0, machine);
        w(&mut bytes, 2, sections.len() as u16);
    }
    for (i, section) in sections.iter().enumerate() {
        let at = header + i * 40;
        let name = if section.name.len() > 8 {
            let offset = strings.len();
            strings.extend(section.name.as_bytes());
            strings.push(0);
            format!("/{offset}")
        } else {
            section.name.to_string()
        };
        bytes[at..at + name.len()].copy_from_slice(name.as_bytes());
        d(&mut bytes, at + 16, section.size);
        if !section.data.is_empty() {
            let offset = bytes.len() as u32;
            d(&mut bytes, at + 20, offset);
            bytes.extend(&section.data);
        }
        if !section.relocations.is_empty() {
            let offset = bytes.len() as u32;
            d(&mut bytes, at + 24, offset);
            let extended = section.relocations.len() > 65535;
            w(
                &mut bytes,
                at + 32,
                if extended {
                    65535
                } else {
                    section.relocations.len() as u16
                },
            );
            if extended {
                bytes.extend((section.relocations.len() as u32 + 1).to_le_bytes());
                bytes.extend([0; 6]);
            }
            for &(offset, index, typ) in &section.relocations {
                bytes.extend(offset.to_le_bytes());
                bytes.extend(index.to_le_bytes());
                bytes.extend(typ.to_le_bytes());
            }
            d(
                &mut bytes,
                at + 36,
                section.flags | if extended { 0x01000000 } else { 0 },
            );
        } else {
            d(&mut bytes, at + 36, section.flags);
        }
    }
    let offset = bytes.len() as u32;
    let count = symbols.iter().map(|s| 1 + s.auxiliary.len()).sum::<usize>() as u32;
    d(&mut bytes, if big { 48 } else { 8 }, offset);
    d(&mut bytes, if big { 52 } else { 12 }, count);
    for symbol in symbols {
        let mut row = vec![0u8; record];
        if symbol.name.len() > 8 {
            d(&mut row, 4, strings.len() as u32);
            strings.extend(symbol.name.as_bytes());
            strings.push(0);
        } else {
            row[..symbol.name.len()].copy_from_slice(symbol.name.as_bytes());
        }
        d(&mut row, 8, symbol.value);
        if big {
            d(&mut row, 12, symbol.section as u32);
        } else {
            w(&mut row, 12, symbol.section as u16);
        }
        row[record - 2] = symbol.storage;
        row[record - 1] = symbol.auxiliary.len() as u8;
        bytes.extend(row);
        for aux in &symbol.auxiliary {
            let mut row = aux.clone();
            row.resize(record, 0);
            bytes.extend(row);
        }
    }
    let len = strings.len() as u32;
    d(&mut strings, 0, len);
    bytes.extend(strings);
    bytes
}

pub fn archive(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = b"!<arch>\n".to_vec();
    for &(name, data) in members {
        let header = format!(
            "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
            name,
            0,
            0,
            0,
            "100644",
            data.len()
        );
        assert_eq!(header.len(), 60);
        out.extend(header.as_bytes());
        out.extend(data);
        if out.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
    out
}
pub fn zip(members: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    use std::io::Write;
    fn w(out: &mut [u8], at: usize, n: u16) {
        out[at..at + 2].copy_from_slice(&n.to_le_bytes());
    }
    fn d(out: &mut [u8], at: usize, n: u32) {
        out[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for &(name, data) in members {
        let compressed = if deflate {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(data).unwrap();
            encoder.finish().unwrap()
        } else {
            data.to_vec()
        };
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        let method = if deflate { 8 } else { 0 };
        let mut local = vec![0; 30];
        local[..4].copy_from_slice(b"PK\x03\x04");
        w(&mut local, 4, 20);
        w(&mut local, 8, method);
        d(&mut local, 14, crc);
        d(&mut local, 18, compressed.len() as u32);
        d(&mut local, 22, data.len() as u32);
        w(&mut local, 26, name.len() as u16);
        out.extend(local);
        out.extend(name.as_bytes());
        out.extend(compressed);
        let mut central = vec![0; 46];
        central[..4].copy_from_slice(b"PK\x01\x02");
        w(&mut central, 4, 20);
        w(&mut central, 6, 20);
        w(&mut central, 10, method);
        d(&mut central, 16, crc);
        d(
            &mut central,
            20,
            (out.len() - offset as usize - 30 - name.len()) as u32,
        );
        d(&mut central, 24, data.len() as u32);
        w(&mut central, 28, name.len() as u16);
        d(&mut central, 42, offset);
        directory.extend(central);
        directory.extend(name.as_bytes());
    }
    let offset = out.len() as u32;
    let length = directory.len() as u32;
    out.extend(directory);
    let mut footer = vec![0; 22];
    footer[..4].copy_from_slice(b"PK\x05\x06");
    w(&mut footer, 8, members.len() as u16);
    w(&mut footer, 10, members.len() as u16);
    d(&mut footer, 12, length);
    d(&mut footer, 16, offset);
    out.extend(footer);
    out
}

pub fn section_symbol<'a>(
    name: &'a str,
    section: i32,
    size: u32,
    selection: u8,
    parent: u32,
) -> Symbol<'a> {
    let mut auxiliary = vec![0; 18];
    auxiliary[..4].copy_from_slice(&size.to_le_bytes());
    auxiliary[12..14].copy_from_slice(&(parent as u16).to_le_bytes());
    auxiliary[14] = selection;
    auxiliary[16..18].copy_from_slice(&((parent >> 16) as u16).to_le_bytes());
    Symbol {
        name,
        value: 0,
        section,
        storage: 3,
        auxiliary: vec![auxiliary],
    }
}
