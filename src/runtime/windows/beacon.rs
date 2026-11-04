//! x64 variadic bridges spill optional register arguments into ABI home slots,
//! then tail-call Rust with a va_list covering register and stack arguments.
#![allow(non_snake_case)]
use std::{
    ffi::{c_char, c_void},
    ptr,
};
use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_ACP};

core::arch::global_asm!(
    ".text",
    ".globl b2e_BeaconPrintf",
    "b2e_BeaconPrintf:",
    "mov [rsp + 24], r8",
    "mov [rsp + 32], r9",
    "lea r8, [rsp + 24]",
    "jmp b2e_printf_impl",
    ".globl b2e_BeaconFormatPrintf",
    "b2e_BeaconFormatPrintf:",
    "mov [rsp + 24], r8",
    "mov [rsp + 32], r9",
    "lea r8, [rsp + 24]",
    "jmp b2e_format_printf_impl",
);
unsafe extern "C" {
    fn b2e_BeaconPrintf();
    fn b2e_BeaconFormatPrintf();
}
#[link(name = "msvcrt", kind = "raw-dylib")]
unsafe extern "C" {
    fn _vscprintf(format: *const c_char, args: *const c_char) -> i32;
    fn _vsnprintf(
        buffer: *mut c_char,
        size: usize,
        format: *const c_char,
        args: *const c_char,
    ) -> i32;
    fn calloc(count: usize, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}
#[repr(C)]
struct Buffer {
    original: *mut u8,
    buffer: *mut u8,
    length: i32,
    size: i32,
}

extern "C" fn BeaconDataParse(parser: *mut Buffer, data: *mut u8, size: i32) {
    // Native callers must supply valid pointers and lengths, as with the public C API.
    unsafe {
        if parser.is_null() {
            return;
        }
        ptr::write_bytes(parser, 0, 1);
        if data.is_null() || !(4..=crate::MAX_INPUT as i32).contains(&size) {
            return;
        }
        let length = ptr::read_unaligned(data.cast::<u32>());
        if length > (size - 4) as u32 {
            return;
        }
        (*parser).original = data;
        (*parser).buffer = data.add(4);
        (*parser).length = length as i32;
        (*parser).size = length as i32;
    }
}
extern "C" fn BeaconDataPtr(parser: *mut Buffer, size: i32) -> *mut u8 {
    unsafe {
        if parser.is_null() || (*parser).buffer.is_null() || size < 0 || size > (*parser).length {
            return ptr::null_mut();
        }
        let data = (*parser).buffer;
        (*parser).buffer = data.add(size as usize);
        (*parser).length -= size;
        data
    }
}
extern "C" fn BeaconDataInt(parser: *mut Buffer) -> i32 {
    let data = BeaconDataPtr(parser, 4);
    if data.is_null() {
        0
    } else {
        unsafe { ptr::read_unaligned(data.cast()) }
    }
}
extern "C" fn BeaconDataShort(parser: *mut Buffer) -> i16 {
    let data = BeaconDataPtr(parser, 2);
    if data.is_null() {
        0
    } else {
        unsafe { ptr::read_unaligned(data.cast()) }
    }
}
extern "C" fn BeaconDataLength(parser: *const Buffer) -> i32 {
    if parser.is_null() {
        0
    } else {
        unsafe { (*parser).length }
    }
}
extern "C" fn BeaconDataExtract(parser: *mut Buffer, size: *mut i32) -> *mut u8 {
    unsafe {
        if !size.is_null() {
            *size = 0;
        }
        if BeaconDataLength(parser) < 4 {
            return ptr::null_mut();
        }
        let length = BeaconDataInt(parser);
        let data = BeaconDataPtr(parser, length);
        if !data.is_null() && !size.is_null() {
            *size = length;
        }
        data
    }
}
extern "C" fn BeaconOutput(kind: i32, data: *const c_char, size: i32) {
    if (0..=crate::MAX_INPUT as i32).contains(&size) {
        super::bof2exe_output(kind, data, size);
    }
}
extern "C" fn BeaconDownload(filename: *const c_char, data: *const c_char, length: u32) -> i32 {
    fn copy_download(
        filename: *const c_char,
        data: *const c_char,
        length: u32,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(!filename.is_null(), "null download filename");
        anyhow::ensure!(
            length as usize <= crate::MAX_INPUT,
            "download exceeds capture limit"
        );
        anyhow::ensure!(length == 0 || !data.is_null(), "null download data");
        let mut name = Vec::new();
        // The native caller must supply a valid NUL-terminated filename and readable buffer.
        for index in 0..=crate::downloads::MAX_NAME {
            let byte = unsafe { filename.add(index).read() } as u8;
            if byte == 0 {
                let filename = std::str::from_utf8(&name)?;
                let bytes = if length == 0 {
                    &[]
                } else {
                    unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length as usize) }
                };
                let event = crate::downloads::capture(filename, bytes)?;
                return Ok(super::capture_download(event));
            }
            name.push(byte);
        }
        anyhow::bail!("download filename exceeds 255 UTF-8 bytes")
    }
    match copy_download(filename, data, length) {
        Ok(true) => 1,
        result => {
            let message = match result {
                Err(error) => format!("BeaconDownload: {error}\n"),
                _ => "BeaconDownload: no active capture or capture limit reached\n".into(),
            };
            BeaconOutput(13, message.as_ptr().cast(), message.len() as i32);
            0
        }
    }
}
fn render(format: *const c_char, args: *const c_char) -> Option<Vec<u8>> {
    if format.is_null() {
        return None;
    }
    unsafe {
        let length = _vscprintf(format, args);
        if !(0..crate::MAX_INPUT as i32).contains(&length) {
            return None;
        }
        let mut data = vec![0; length as usize + 1];
        let written = _vsnprintf(data.as_mut_ptr().cast(), data.len(), format, args);
        if written < 0 {
            return None;
        }
        data.truncate(written as usize);
        Some(data)
    }
}
#[no_mangle]
extern "C" fn b2e_printf_impl(kind: i32, format: *const c_char, args: *const c_char) {
    if let Some(data) = render(format, args) {
        BeaconOutput(kind, data.as_ptr().cast(), data.len() as i32);
    }
}
extern "C" fn BeaconFormatAlloc(format: *mut Buffer, size: i32) {
    unsafe {
        if format.is_null() {
            return;
        }
        ptr::write_bytes(format, 0, 1);
        if !(1..=crate::MAX_INPUT as i32).contains(&size) {
            return;
        }
        let data: *mut u8 = calloc(size as usize + 1, 1).cast();
        if !data.is_null() {
            *format = Buffer {
                original: data,
                buffer: data,
                length: 0,
                size,
            };
        }
    }
}
extern "C" fn BeaconFormatReset(format: *mut Buffer) {
    unsafe {
        if format.is_null() || (*format).original.is_null() {
            return;
        }
        ptr::write_bytes((*format).original, 0, (*format).size as usize + 1);
        (*format).buffer = (*format).original;
        (*format).length = 0;
    }
}
extern "C" fn BeaconFormatFree(format: *mut Buffer) {
    unsafe {
        if format.is_null() {
            return;
        }
        free((*format).original.cast());
        ptr::write_bytes(format, 0, 1);
    }
}
extern "C" fn BeaconFormatAppend(format: *mut Buffer, data: *const u8, size: i32) {
    unsafe {
        if format.is_null()
            || (*format).original.is_null()
            || data.is_null()
            || size < 0
            || size > (*format).size - (*format).length
        {
            return;
        }
        ptr::copy(data, (*format).buffer, size as usize);
        (*format).buffer = (*format).buffer.add(size as usize);
        (*format).length += size;
        *(*format).buffer = 0;
    }
}
#[no_mangle]
extern "C" fn b2e_format_printf_impl(format: *mut Buffer, fmt: *const c_char, args: *const c_char) {
    if let Some(data) = render(fmt, args) {
        BeaconFormatAppend(format, data.as_ptr(), data.len() as i32);
    }
}
extern "C" fn BeaconFormatToString(format: *mut Buffer, size: *mut i32) -> *mut u8 {
    unsafe {
        if !size.is_null() {
            *size = if format.is_null() {
                0
            } else {
                (*format).length
            };
        }
        if format.is_null() {
            ptr::null_mut()
        } else {
            (*format).original
        }
    }
}
extern "C" fn BeaconFormatInt(format: *mut Buffer, value: i32) {
    BeaconFormatAppend(format, value.to_be_bytes().as_ptr(), 4);
}
extern "C" fn toWideChar(src: *const u8, dst: *mut u16, max: i32) -> i32 {
    if src.is_null() || dst.is_null() || max < 2 {
        return 0;
    }
    unsafe { i32::from(MultiByteToWideChar(CP_ACP, 0, src, -1, dst, max / 2) != 0) }
}
pub(super) fn resolve(name: &str) -> Option<usize> {
    Some(match name {
        "BeaconDataParse" => BeaconDataParse as *const () as usize,
        "BeaconDataPtr" => BeaconDataPtr as *const () as usize,
        "BeaconDataInt" => BeaconDataInt as *const () as usize,
        "BeaconDataShort" => BeaconDataShort as *const () as usize,
        "BeaconDataLength" => BeaconDataLength as *const () as usize,
        "BeaconDataExtract" => BeaconDataExtract as *const () as usize,
        "BeaconOutput" => BeaconOutput as *const () as usize,
        "BeaconDownload" => BeaconDownload as *const () as usize,
        "BeaconPrintf" => b2e_BeaconPrintf as *const () as usize,
        "BeaconFormatAlloc" => BeaconFormatAlloc as *const () as usize,
        "BeaconFormatReset" => BeaconFormatReset as *const () as usize,
        "BeaconFormatFree" => BeaconFormatFree as *const () as usize,
        "BeaconFormatAppend" => BeaconFormatAppend as *const () as usize,
        "BeaconFormatPrintf" => b2e_BeaconFormatPrintf as *const () as usize,
        "BeaconFormatToString" => BeaconFormatToString as *const () as usize,
        "BeaconFormatInt" => BeaconFormatInt as *const () as usize,
        "toWideChar" => toWideChar as *const () as usize,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn start_capture() {
        super::super::OUTPUT.with(|out| {
            *out.borrow_mut() = super::super::Capture {
                active: true,
                ..Default::default()
            };
        });
    }
    fn take_capture() -> super::super::Capture {
        super::super::OUTPUT.with(|out| std::mem::take(&mut *out.borrow_mut()))
    }
    #[test]
    fn download_bridge_resolves_abi_and_copies_the_buffer() {
        start_capture();
        let function: extern "C" fn(*const c_char, *const c_char, u32) -> i32 =
            unsafe { std::mem::transmute(resolve("BeaconDownload").unwrap()) };
        let mut bytes = [0u8, 255, 1, 0];
        assert_eq!(
            function(
                c"payload.bin".as_ptr(),
                bytes.as_ptr().cast(),
                bytes.len() as u32
            ),
            1
        );
        bytes.fill(42);
        assert_eq!(function(c"empty.bin".as_ptr(), ptr::null(), 0), 1);
        let capture = take_capture();
        assert!(!capture.overflow);
        assert_eq!(capture.events.len(), 2);
        let download = crate::downloads::decode(&capture.events[0])
            .unwrap()
            .unwrap();
        assert_eq!(download.filename, "payload.bin");
        assert_eq!(download.data, &[0, 255, 1, 0]);
        assert!(crate::downloads::decode(&capture.events[1])
            .unwrap()
            .unwrap()
            .data
            .is_empty());
    }
    #[test]
    fn download_bridge_rejects_invalid_inputs_limits_and_unregistered_threads() {
        start_capture();
        assert_eq!(BeaconDownload(ptr::null(), ptr::null(), 0), 0);
        assert_eq!(BeaconDownload(c"data.bin".as_ptr(), ptr::null(), 1), 0);
        assert_eq!(BeaconDownload(c"../escape".as_ptr(), c"x".as_ptr(), 1), 0);
        assert_eq!(
            BeaconDownload(
                c"data.bin".as_ptr(),
                c"x".as_ptr(),
                crate::MAX_INPUT as u32 + 1
            ),
            0
        );
        let long = std::ffi::CString::new("x".repeat(256)).unwrap();
        assert_eq!(BeaconDownload(long.as_ptr(), ptr::null(), 0), 0);
        let invalid_utf8 = [255u8, 0];
        assert_eq!(
            BeaconDownload(invalid_utf8.as_ptr().cast(), ptr::null(), 0),
            0
        );
        let capture = take_capture();
        assert_eq!(capture.events.len(), 6);
        assert!(capture.events.iter().all(|event| event.kind == 13));

        start_capture();
        let returned =
            std::thread::spawn(|| BeaconDownload(c"worker.bin".as_ptr(), c"x".as_ptr(), 1))
                .join()
                .unwrap();
        assert_eq!(returned, 0);
        assert!(take_capture().events.is_empty());
        start_capture();
        super::super::OUTPUT.with(|out| out.borrow_mut().bytes = crate::MAX_INPUT);
        assert_eq!(BeaconDownload(c"limit.bin".as_ptr(), c"x".as_ptr(), 1), 0);
        assert!(take_capture().overflow);
    }
    fn empty() -> Buffer {
        Buffer {
            original: ptr::null_mut(),
            buffer: ptr::null_mut(),
            length: 0,
            size: 0,
        }
    }
    #[test]
    fn parser_bounds_and_format_capacity() {
        let mut p = empty();
        let mut data = [2u8, 0, 0, 0, 254, 255];
        BeaconDataParse(&mut p, data.as_mut_ptr(), data.len() as i32);
        assert_eq!(BeaconDataLength(&p), 2);
        assert_eq!(BeaconDataShort(&mut p), -2);
        assert_eq!(BeaconDataInt(&mut p), 0);
        assert!(BeaconDataPtr(&mut p, -1).is_null());
        data[0] = 255;
        BeaconDataParse(&mut p, data.as_mut_ptr(), data.len() as i32);
        assert_eq!(BeaconDataLength(&p), 0);

        let mut f = empty();
        BeaconFormatAlloc(&mut f, 4);
        BeaconFormatInt(&mut f, 0x01020304);
        BeaconFormatAppend(&mut f, b"too much".as_ptr(), 8);
        let mut length = 0;
        let bytes = BeaconFormatToString(&mut f, &mut length);
        assert_eq!(length, 4);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(bytes, 5) },
            &[1, 2, 3, 4, 0]
        );
        BeaconFormatReset(&mut f);
        assert_eq!(f.length, 0);
        BeaconFormatFree(&mut f);
        assert!(f.original.is_null());
    }
    #[test]
    fn variadic_format_bridge_handles_float_and_stack_arguments() {
        let mut f = empty();
        BeaconFormatAlloc(&mut f, 128);
        let function: unsafe extern "C" fn(*mut Buffer, *const c_char, ...) =
            unsafe { std::mem::transmute(resolve("BeaconFormatPrintf").unwrap()) };
        unsafe {
            function(
                &mut f,
                c"%d %.1f %d %s".as_ptr(),
                42i32,
                1.5f64,
                17i32,
                c"ok".as_ptr(),
            );
        }
        let bytes = BeaconFormatToString(&mut f, ptr::null_mut());
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(bytes.cast()) }.to_bytes(),
            b"42 1.5 17 ok"
        );
        BeaconFormatFree(&mut f);
    }
    #[test]
    fn advertised_symbols_exist() {
        for name in crate::inspect::BEACON_APIS {
            assert!(resolve(name).is_some(), "{name}");
        }
        assert!(resolve("BeaconUnknown").is_none());
    }
}
