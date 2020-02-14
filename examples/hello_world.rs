#![no_std]
#![no_main]
#![feature(start)]

use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::{debug::SvcDebugLog, entry, env};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog::default();
    let _ = writeln!(log, "{}", info);

    ctru_rt::svc::exit_process()
}

entry!(main);

fn main() {
    let mut log = SvcDebugLog::default();

    writeln!(log, "Hello, World!, app_id is {:#0x}", env::app_id())
        .expect("Failed to write Hello World");
}
