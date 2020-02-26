#![no_std]
#![no_main]

use core::time::Duration;
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
    let app_mem_used = os::MemoryRegion::Application
        .used()
        .expect("Failed to get memory information");
    writeln!(
        log,
        "Hello, World!, app_id is {:#0x}, app mem used: {:#0x}",
        app_id, app_mem_used
    )
    .expect("Failed to write Hello World");

    let _ = ctru_rt::svc::sleep_thread(Duration::from_secs(2));

    let _ = writeln!(log, "Bye-bye!");
}
