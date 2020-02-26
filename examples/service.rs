#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::{debug::SvcDebugLog, entry, result::Result, services, svc};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog::default();
    let _ = writeln!(log, "{}", info);

    ctru_rt::svc::exit_process()
}

entry!(main);

fn run(log: &mut SvcDebugLog) -> Result<()> {
    let srv = services::srv::Srv::init()?;

    let _ = writeln!(log, "Initialized srv: {:#?}", srv);

    let handle = srv.get_service_handle("news:s")?;

    let _ = writeln!(log, "Got news:s service handle: {:08x?}", handle);

    svc::close_handle(handle)?;

    Ok(())
}

fn main() {
    let mut log = SvcDebugLog::default();

    match run(&mut log) {
        Ok(_) => {}
        Err(e) => {
            let _ = writeln!(log, "Failed to run: {:#x?}", e);
        }
    }
}
