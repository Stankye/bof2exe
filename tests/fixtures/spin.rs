#![no_std]
#[no_mangle]
pub extern "C" fn go(_: *mut u8, _: i32) { loop { core::hint::spin_loop(); } }
