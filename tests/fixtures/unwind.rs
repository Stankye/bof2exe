#![no_std]
use core::ffi::{c_char,c_void};
unsafe extern "C" {
    fn BeaconPrintf(kind:i32,format:*const c_char,...);
    #[link_name="__imp_KERNEL32$RtlLookupFunctionEntry"]
    static Lookup:unsafe extern "system" fn(u64,*mut u64,*mut c_void)->*mut c_void;
}
#[no_mangle]
pub unsafe extern "C" fn go(_: *mut u8,_:i32) {
    let mut base=0u64;
    unsafe {
        let record=Lookup(go as *const () as u64,&mut base,core::ptr::null_mut());
        BeaconPrintf(0,c"unwind=%d\n".as_ptr(),i32::from(!record.is_null() && base!=0));
    }
}
