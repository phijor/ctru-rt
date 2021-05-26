#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo};

use log::{error, info};

use ctru_rt::{
    debug::SvcDebugLog,
    entry,
    result::Result,
    services,
    svc::{sleep_thread, Timeout, UserBreakReason},
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog::default();
    let _ = writeln!(log, "[PANIC] {}", info);

    ctru_rt::svc::user_break(UserBreakReason::Panic)
}

fn run() -> Result<()> {
    let srv = services::srv::Srv::init()?;

    let _ = info!("Initialized srv: {:#0x?}", srv);

    // let buffer = PageAlignedBuffer::allocate(0x4000).map_err(|e| match e {
    //     PageAlignErr::Alloc => {
    //         ErrorCode::new(Level::Fatal, Summary::OutOfResource, Module::Os, 1011)
    //     }
    //     PageAlignErr::Layout(_) => {
    //         ErrorCode::new(Level::Fatal, Summary::OutOfResource, Module::Os, 1009)
    //     }
    // })?;
    // let soc = services::soc::Soc::init(&srv, buffer)?;
    // let _ = info!("Initialized soc: {:#0x?}", soc);

    // let socket = soc.socket(2, 1, 0)?;
    // let _ = info!("Initialized socket: {:#0x?}", socket);

    let hid = services::hid::Hid::init(&srv)?;

    loop {
        let kpad = hid.last_keypad();

        info!("kpad = {:?}", kpad);

        sleep_thread(Timeout::from_seconds(1));
    }
}

#[entry]
fn main() {
    let _ = ctru_rt::debug::init_log();

    match run() {
        Ok(_) => {}
        Err(e) => {
            let _ = error!("Failed to run: {:?}", e);
        }
    }
}
