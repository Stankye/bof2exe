#![no_std]
use core::ffi::c_char;
#[repr(C)]
struct Parser {original:*mut u8,buffer:*mut u8,length:i32,size:i32}
unsafe extern "C" {
    fn BeaconDataParse(parser:*mut Parser,data:*mut u8,size:i32);
    fn BeaconDataInt(parser:*mut Parser)->i32;
    fn BeaconDataExtract(parser:*mut Parser,size:*mut i32)->*const c_char;
    fn BeaconPrintf(kind:i32,format:*const c_char,...);
}
#[no_mangle]
pub unsafe extern "C" fn go(data:*mut u8,size:i32) {
    let mut parser=Parser{original:core::ptr::null_mut(),buffer:core::ptr::null_mut(),length:0,size:0};
    unsafe {
        BeaconDataParse(&mut parser,data,size);let count=BeaconDataInt(&mut parser);let name=BeaconDataExtract(&mut parser,core::ptr::null_mut());
        if name.is_null(){BeaconPrintf(13,c"A name is required.\n".as_ptr());return;}
        BeaconPrintf(0,c"Hello, %s! Count: %d\n".as_ptr(),name,count);
    }
}
