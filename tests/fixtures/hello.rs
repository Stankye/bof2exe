#![no_std]
use core::ffi::c_char;
#[repr(C)]
struct Parser { original: *mut u8, buffer: *mut u8, length: i32, size: i32 }
unsafe extern "C" {
    fn BeaconDataParse(parser: *mut Parser, data: *mut u8, size: i32);
    fn BeaconDataInt(parser: *mut Parser) -> i32;
    fn BeaconDataExtract(parser: *mut Parser, size: *mut i32) -> *const c_char;
    fn BeaconPrintf(kind: i32, format: *const c_char, ...);
    #[link_name = "__imp_KERNEL32$GetCurrentProcessId"]
    static GetCurrentProcessId: unsafe extern "system" fn() -> u32;
}
static mut COUNTER: i32 = 0;
#[no_mangle]
pub unsafe extern "C" fn go(data: *mut u8, size: i32) {
    let mut parser = Parser { original: core::ptr::null_mut(), buffer: core::ptr::null_mut(), length: 0, size: 0 };
    unsafe {
        BeaconDataParse(&mut parser, data, size);
        let count = BeaconDataInt(&mut parser);
        let name = BeaconDataExtract(&mut parser, core::ptr::null_mut());
        if name.is_null() {
            BeaconPrintf(13, b"expected int and str arguments\n\0".as_ptr().cast()); return;
        }
        COUNTER += 1;
        // Six variadic arguments exercise register homes and stack arguments.
        BeaconPrintf(0, b"hello %s: count=%d bss=%d pid=%lu extra=%d %d\n\0".as_ptr().cast(),
            name, count, COUNTER, GetCurrentProcessId(), 17i32, 23i32);
    }
}
