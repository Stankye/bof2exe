#![no_std]
unsafe extern "C" { fn BeaconOutput(kind:i32,data:*const u8,size:i32); }
#[no_mangle]
pub unsafe extern "C" fn go(_: *mut u8,_:i32) {
    let mut data=[0u8;16384];
    unsafe {
        core::ptr::write_volatile(data.as_mut_ptr(),17);
        core::ptr::write_volatile(data.as_mut_ptr().add(16383),23);
        BeaconOutput(0,data.as_ptr(),16384);
    }
}
