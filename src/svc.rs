pub fn output_debug_string(message: &str) {
    extern "C" {
        fn svcOutputDebugString(message: *const u8, length: u32);
    }

    unsafe { svcOutputDebugString(message.as_ptr(), message.len() as u32) }
}
