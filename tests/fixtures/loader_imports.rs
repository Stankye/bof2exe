#![no_std]
use core::ffi::c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetModuleHandleA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *const c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}
unsafe extern "C" {
    fn BeaconOutput(kind: i32, data: *const u8, length: i32);
}

#[no_mangle]
pub unsafe extern "C" fn go(_: *mut u8, _: i32) {
    let name = b"kernel32.dll\0";
    let module = LoadLibraryA(name.as_ptr());
    let passed = !module.is_null()
        && GetModuleHandleA(name.as_ptr()) == module
        && !GetProcAddress(module, b"GetCurrentProcessId\0".as_ptr()).is_null();
    let freed = !module.is_null() && FreeLibrary(module) != 0;
    if passed && freed {
        BeaconOutput(0, b"loader-imports=ok\n".as_ptr(), 18);
    } else {
        BeaconOutput(13, b"loader-imports=failed\n".as_ptr(), 22);
    }
}
