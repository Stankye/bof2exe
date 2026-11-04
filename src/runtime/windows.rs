use super::Output;
use crate::{
    coff::Coff,
    inspect::{import_name, inspect},
};
use anyhow::{bail, ensure, Context, Result};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{c_char, CString},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::{
        Diagnostics::Debug::{
            FlushInstructionCache, RtlAddFunctionTable, RtlDeleteFunctionTable,
            IMAGE_RUNTIME_FUNCTION_ENTRY,
        },
        LibraryLoader::{GetProcAddress, LoadLibraryExA, LOAD_LIBRARY_SEARCH_SYSTEM32},
        Memory::{
            VirtualAlloc, VirtualFree, VirtualProtect, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
            PAGE_EXECUTE_READ, PAGE_READONLY, PAGE_READWRITE,
        },
        Threading::GetCurrentProcess,
    },
};
mod beacon;

core::arch::global_asm!(
    ".text",
    ".globl b2e_chkstk",
    "b2e_chkstk:",
    "push rcx",
    "push rax",
    "lea rcx, [rsp + 24]",
    "2:",
    "cmp rax, 4096",
    "jb 3f",
    "sub rcx, 4096",
    "test byte ptr [rcx], 0",
    "sub rax, 4096",
    "jmp 2b",
    "3:",
    "sub rcx, rax",
    "test byte ptr [rcx], 0",
    "pop rax",
    "pop rcx",
    "ret",
);
unsafe extern "C" {
    fn b2e_chkstk();
}

#[derive(Default)]
struct Capture {
    events: Vec<Output>,
    bytes: usize,
    overflow: bool,
    active: bool,
}
thread_local! { static OUTPUT: RefCell<Capture> = const { RefCell::new(Capture { events: Vec::new(), bytes: 0, overflow: false, active: false }) }; }

fn capture_download(event: Output) -> bool {
    OUTPUT.with(|slot| {
        let mut out = slot.borrow_mut();
        // An unregistered worker thread has no executing-call capture to receive the file.
        if !out.active {
            return false;
        }
        if out.bytes.saturating_add(event.data.len()) > crate::MAX_INPUT
            || out.events.len() >= 65536
        {
            out.overflow = true;
            return false;
        }
        if out.events.try_reserve(1).is_err() {
            out.overflow = true;
            return false;
        }
        out.bytes += event.data.len();
        out.events.push(event);
        true
    })
}
#[no_mangle]
extern "C" fn bof2exe_output(kind: i32, data: *const c_char, length: i32) {
    if data.is_null() || length < 0 {
        return;
    }
    OUTPUT.with(|slot| {
        let mut out = slot.borrow_mut();
        if out.bytes.saturating_add(length as usize) > crate::MAX_INPUT || out.events.len() >= 65536
        {
            out.overflow = true;
            return;
        }
        // SAFETY: BeaconOutput's native caller supplies a valid readable buffer.
        let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length as usize) };
        out.bytes += bytes.len();
        out.events.push(Output {
            kind,
            data: bytes.to_vec(),
        });
    });
}

struct Mapping {
    base: *mut u8,
    size: usize,
}
impl Mapping {
    fn new(size: usize) -> Result<Self> {
        // SAFETY: reserves a private, initially writable allocation of the requested size.
        let base: *mut u8 =
            unsafe { VirtualAlloc(ptr::null(), size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE) }
                .cast();
        ensure!(
            !base.is_null(),
            "VirtualAlloc: {}",
            std::io::Error::last_os_error()
        );
        Ok(Self { base, size })
    }
    fn address(&self, offset: usize) -> usize {
        self.base as usize + offset
    }
    fn bytes(&mut self) -> &mut [u8] {
        // SAFETY: this uniquely borrowed mapping owns size bytes of committed memory.
        unsafe { std::slice::from_raw_parts_mut(self.base, self.size) }
    }
    fn protect(&self, offset: usize, len: usize, flags: u32) -> Result<()> {
        let mut old = 0;
        // SAFETY: all callers use page-aligned ranges within this mapping.
        ensure!(
            unsafe { VirtualProtect(self.base.add(offset).cast(), len, flags, &mut old) } != 0,
            "VirtualProtect: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            VirtualFree(self.base.cast(), 0, MEM_RELEASE);
        }
    }
}
#[derive(Default)]
struct Libraries(BTreeMap<String, HMODULE>);
impl Drop for Libraries {
    fn drop(&mut self) {
        for handle in self.0.values() {
            unsafe {
                FreeLibrary(*handle);
            }
        }
    }
}
impl Libraries {
    fn resolve(&mut self, name: &str) -> Result<usize> {
        if let Some(address) = beacon::resolve(name) {
            return Ok(address);
        }
        if crate::inspect::COMPILER_APIS.contains(&name) {
            return Ok(b2e_chkstk as *const () as usize);
        }
        if crate::inspect::CRT_APIS.contains(&name) {
            return self.resolve(&format!("msvcrt${name}"));
        }
        if crate::inspect::LOADER_APIS.contains(&name) {
            return self.resolve(&format!("kernel32${name}"));
        }
        ensure!(
            crate::inspect::valid_dll_import(name),
            "unsupported external {name}"
        );
        let (dll, export) = name.split_once('$').context("invalid DLL import")?;
        let dll = if dll.to_ascii_lowercase().ends_with(".dll") {
            dll.to_ascii_lowercase()
        } else {
            format!("{}.dll", dll.to_ascii_lowercase())
        };
        if !self.0.contains_key(&dll) {
            let c_dll = CString::new(dll.as_str())?;
            // Restrict implicit DLL loading to System32 instead of searching the current directory.
            let handle = unsafe {
                LoadLibraryExA(
                    c_dll.as_ptr().cast(),
                    ptr::null_mut(),
                    LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
            };
            ensure!(
                !handle.is_null(),
                "load {dll}: {}",
                std::io::Error::last_os_error()
            );
            self.0.insert(dll.clone(), handle);
        }
        let export_name = CString::new(export)?;
        let address = unsafe { GetProcAddress(self.0[&dll], export_name.as_ptr().cast()) }
            .with_context(|| format!("export {export} was not found in {dll}"))?;
        Ok(address as *const () as usize)
    }
}
fn pages(size: usize) -> usize {
    size.max(1).div_ceil(4096) * 4096
}

pub(super) unsafe fn execute(
    bytes: &[u8],
    entry: &str,
    arguments: &mut [u8],
) -> Result<Vec<Output>> {
    let prepared = crate::link::prepare(bytes)?;
    let bytes = prepared.as_ref();
    let report = inspect(bytes, entry)?;
    ensure!(
        report.compatible(),
        "incompatible object:\n{}",
        report.issues.join("\n")
    );
    ensure!(
        arguments.len() <= crate::MAX_INPUT,
        "arguments exceed 64 MiB"
    );
    ensure!(
        arguments.len() >= 4
            && u32::from_le_bytes(arguments[..4].try_into()?) as usize == arguments.len() - 4,
        "arguments must contain a valid Beacon length header"
    );
    let file = Coff::parse(bytes)?;
    let mut offsets = Vec::new();
    let mut total = 0;
    for section in &file.sections {
        offsets.push(total);
        total += pages(section.size);
    }
    let imports_offset = total;
    let imports_size = pages(
        report
            .imports
            .len()
            .checked_mul(32)
            .context("import table overflow")?,
    );
    ensure!(
        total.saturating_add(imports_size) <= crate::MAX_INPUT,
        "mapped object exceeds 64 MiB"
    );
    let mut memory = Mapping::new(total + imports_size)?;
    for (section, &offset) in file.sections.iter().zip(&offsets) {
        memory.bytes()[offset..offset + section.data.len()].copy_from_slice(section.data);
    }
    let mut libraries = Libraries::default();
    let mut externals = BTreeMap::new();
    for (i, name) in report.imports.iter().enumerate() {
        let resolved = libraries.resolve(import_name(name))?;
        let offset = imports_offset + i * 32;
        memory.bytes()[offset..offset + 8].copy_from_slice(&(resolved as u64).to_le_bytes());
        // Absolute jump stub keeps direct REL32 calls within the object's allocation.
        memory.bytes()[offset + 8..offset + 14].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
        memory.bytes()[offset + 14..offset + 22].copy_from_slice(&(resolved as u64).to_le_bytes());
        externals.insert(name.as_str(), (resolved, offset));
    }
    for (section_index, section) in file.sections.iter().enumerate() {
        for reloc in &section.relocations {
            if reloc.typ == 0 {
                continue;
            }
            let symbol = file.symbols[reloc.symbol]
                .as_ref()
                .context("missing symbol")?;
            let (target, section_number, section_relative) = if symbol.section > 0 {
                let index = symbol.section as usize - 1;
                (
                    memory.address(offsets[index] + symbol.value as usize),
                    symbol.section as u16,
                    symbol.value as u64,
                )
            } else if symbol.section == -1 {
                (symbol.value as usize, 0, symbol.value as u64)
            } else if let Some(&(resolved, offset)) = externals.get(symbol.name.as_str()) {
                let addr = if symbol.name.starts_with("__imp_") {
                    memory.address(offset)
                } else if (4..=9).contains(&reloc.typ) {
                    memory.address(offset + 8)
                } else {
                    resolved
                };
                (addr, 0, 0)
            } else {
                bail!("cannot resolve {}", symbol.name);
            };
            let offset = offsets[section_index] + reloc.offset;
            let place = memory.address(offset);
            let base = memory.base as usize;
            let data = memory.bytes();
            match reloc.typ {
                1 => {
                    let addend = u64::from_le_bytes(data[offset..offset + 8].try_into()?);
                    data[offset..offset + 8]
                        .copy_from_slice(&(target as u64).wrapping_add(addend).to_le_bytes());
                }
                2 | 3 | 11 => {
                    let addend = u32::from_le_bytes(data[offset..offset + 4].try_into()?) as u64;
                    let value = match reloc.typ {
                        2 => target as u64,
                        3 => target
                            .checked_sub(base)
                            .context("ADDR32NB target precedes image")?
                            as u64,
                        _ => {
                            ensure!(section_number != 0, "SECREL requires a section symbol");
                            section_relative
                        }
                    }
                    .checked_add(addend)
                    .context("relocation addition overflow")?;
                    let value = u32::try_from(value).context("32-bit relocation overflow")?;
                    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                4..=9 => {
                    let addend = i32::from_le_bytes(data[offset..offset + 4].try_into()?) as i64;
                    let value = target as i64 + addend - place as i64 - 4 - (reloc.typ - 4) as i64;
                    let value = i32::try_from(value).context("REL32 target is out of range")?;
                    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                10 => {
                    ensure!(section_number != 0, "SECTION requires a section symbol");
                    let addend = u16::from_le_bytes(data[offset..offset + 2].try_into()?);
                    let value = section_number
                        .checked_add(addend)
                        .context("SECTION relocation overflow")?;
                    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
                }
                _ => bail!("unsupported relocation"),
            }
        }
    }
    let unwind = UnwindTables::register(&mut memory, &file, &offsets)?;
    for (section, &offset) in file.sections.iter().zip(&offsets) {
        let flags = if section.executable() {
            PAGE_EXECUTE_READ
        } else if section.writable() {
            PAGE_READWRITE
        } else {
            PAGE_READONLY
        };
        memory.protect(offset, pages(section.size), flags)?;
    }
    memory.protect(imports_offset, imports_size, PAGE_EXECUTE_READ)?;
    ensure!(
        unsafe { FlushInstructionCache(GetCurrentProcess(), memory.base.cast(), memory.size) } != 0,
        "FlushInstructionCache failed"
    );
    let symbol = file
        .symbols
        .iter()
        .flatten()
        .find(|s| s.name == entry && s.section > 0)
        .context("entrypoint missing")?;
    let address = memory.address(offsets[symbol.section as usize - 1] + symbol.value as usize);
    // SAFETY: entry was validated in an executable section; caller guarantees its ABI and behavior.
    let function: unsafe extern "C" fn(*mut u8, i32) = unsafe { std::mem::transmute(address) };
    OUTPUT.with(|out| {
        *out.borrow_mut() = Capture {
            active: true,
            ..Capture::default()
        }
    });
    unsafe {
        function(arguments.as_mut_ptr(), arguments.len() as i32);
    }
    let capture = OUTPUT.with(|out| std::mem::take(&mut *out.borrow_mut()));
    drop(unwind);
    ensure!(
        !capture.overflow,
        "Beacon output exceeded 64 MiB or 65536 callbacks"
    );
    Ok(capture.events)
}

/// Owns the registrations, borrowing no mapped data after its destructor runs.
struct UnwindTables(Vec<*const IMAGE_RUNTIME_FUNCTION_ENTRY>);
impl UnwindTables {
    fn register(memory: &mut Mapping, file: &Coff<'_>, offsets: &[usize]) -> Result<Self> {
        let mut tables = Self(Vec::new());
        for (section, &offset) in file.sections.iter().zip(offsets) {
            if (section.name != ".pdata" && !section.name.starts_with(".pdata$"))
                || section.size == 0
            {
                continue;
            }
            ensure!(section.size % 12 == 0, "invalid AMD64 .pdata size");
            let mut records: Vec<[u8; 12]> = memory.bytes()[offset..offset + section.size]
                .as_chunks::<12>()
                .0
                .to_vec();
            records.sort_by_key(|row| u32::from_le_bytes(row[..4].try_into().unwrap()));
            let mut previous_end = 0;
            for record in &records {
                let begin = u32::from_le_bytes(record[..4].try_into()?) as usize;
                let end = u32::from_le_bytes(record[4..8].try_into()?) as usize;
                let unwind = u32::from_le_bytes(record[8..].try_into()?) as usize;
                ensure!(
                    begin < end && begin >= previous_end,
                    "invalid/overlapping unwind function range"
                );
                ensure!(
                    file.sections
                        .iter()
                        .zip(offsets)
                        .any(|(s, &o)| s.executable() && begin >= o && end <= o + s.size),
                    "unwind function range is outside code"
                );
                ensure!(
                    unwind.is_multiple_of(4)
                        && file.sections.iter().zip(offsets).any(|(s, &o)| unwind >= o
                            && unwind.checked_add(4).is_some_and(|end| end <= o + s.size)),
                    "unwind info is out of bounds or unaligned"
                );
                let header = &memory.bytes()[unwind..unwind + 4];
                ensure!(header[0] & 7 == 1, "unsupported UNWIND_INFO version");
                // A complete validation of language-specific handler data belongs to the module.
                let codes_end = unwind + 4 + (header[2] as usize).div_ceil(2) * 4;
                let flags = header[0] >> 3;
                let trailer = if flags & 4 != 0 {
                    12
                } else if flags & 3 != 0 {
                    4
                } else {
                    0
                };
                ensure!(
                    flags & !7 == 0 && !(flags & 4 != 0 && flags & 3 != 0),
                    "invalid unwind flags"
                );
                ensure!(
                    file.sections
                        .iter()
                        .zip(offsets)
                        .any(|(s, &o)| unwind >= o && codes_end + trailer <= o + s.size),
                    "truncated unwind codes or handler trailer"
                );
                previous_end = end;
            }
            for (index, record) in records.iter().enumerate() {
                memory.bytes()[offset + index * 12..offset + (index + 1) * 12]
                    .copy_from_slice(record);
            }
            let table = memory.address(offset) as *const IMAGE_RUNTIME_FUNCTION_ENTRY;
            // SAFETY: sorted, range-checked records live until this guard is dropped.
            ensure!(
                unsafe { RtlAddFunctionTable(table, records.len() as u32, memory.base as u64) },
                "RtlAddFunctionTable failed"
            );
            tables.0.push(table);
        }
        Ok(tables)
    }
}
impl Drop for UnwindTables {
    fn drop(&mut self) {
        for &table in &self.0 {
            unsafe {
                RtlDeleteFunctionTable(table);
            }
        }
    }
}
