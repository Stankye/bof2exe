#![no_std]
#[no_mangle]
pub extern "C" fn go(_: *mut u8, _: i32) {
    // Deterministic illegal instruction for testing worker fault isolation.
    unsafe { core::arch::asm!("ud2", options(noreturn)); }
}
