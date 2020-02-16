#![no_std]
#![no_main]
#![feature(start)]

use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::{debug::SvcDebugLog, entry, env, os};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog::default();
    let _ = writeln!(log, "{}", info);

    ctru_rt::svc::exit_process()
}

entry!(main);

fn main() {
    let mut log = SvcDebugLog::default();

    let app_id = env::app_id();
    let app_mem_used = os::MemoryRegion::Application.used();
    writeln!(
        log,
        "Hello, World!, app_id is {:#0x}, app mem used: {:#0x}",
        app_id, app_mem_used
    )
    .expect("Failed to write Hello World");
}
