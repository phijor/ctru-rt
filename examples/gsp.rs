#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::{
    entry,
    graphics::Grapics,
    result::Result,
    services::{gsp::gpu::Gpu, hid::Hid, srv::Srv},
    svc,
};
use log::{error, info};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    use ctru_rt::{debug::SvcDebugLog, svc::UserBreakReason};
    let mut log = SvcDebugLog::default();
    let _ = writeln!(log, "[PANIC] {}", info);

    svc::user_break(UserBreakReason::Panic)
}

fn run() -> Result<()> {
    let srv = Srv::init()?;
    info!("Initialized `srv`: {:#0x?}", srv);

    let mut gpu = Gpu::init(&srv)?;
    info!("Initialized `gsp::Gpu`: {:#0x?}", gpu);

    let mut gfx = Grapics::init_default(&mut gpu)?;
    info!("Initialized graphics: {:#0x?}", gfx);

    let hid = Hid::init(&srv)?;

    let mut runs = 0..5;
    info!("Press START to exit");
    while !hid.last_keypad().start() {
        info!("Waiting for VBLANK0...");

        gfx.wait_vblank0().ok();
        if let None = runs.next() {
            break;
        }
    }

    info!("Exiting...");

    Ok(())
}

#[entry]
fn main() {
    let _ = ctru_rt::debug::init_log();

    match run() {
        Ok(_) => {}
        Err(e) => {
            let _ = error!("Failed to run: {:?}", e);
            svc::exit_process()
        }
    }
}
