#![no_std]
use core::ffi::c_char;
unsafe extern "C" {
    fn helper(value:i32)->i32;
    fn BeaconPrintf(kind:i32,format:*const c_char,...);
    fn BeaconOutput(kind:i32,data:*const u8,size:i32);
    static greeting:u8;
    static greeting_len:u32;
}
#[no_mangle]
pub unsafe extern "C" fn go(_: *mut u8,_:i32) {
    unsafe {
        BeaconPrintf(0,c"linked=%d data=".as_ptr(),helper(7));
        BeaconOutput(0,&raw const greeting,greeting_len as i32);
    }
}
