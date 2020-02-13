#![no_std]
#![no_main]
#![feature(start)]

use core::panic::PanicInfo;

use ctru_rt::svc::output_debug_string;

#[panic_handler]
fn panic_handler(_: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub fn start() {
    output_debug_string("Hello, World!");

    loop {}
}
