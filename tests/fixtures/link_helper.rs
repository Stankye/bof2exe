#![no_std]
#[no_mangle]
pub extern "C" fn helper(value:i32)->i32 { value.wrapping_mul(3).wrapping_add(1) }
