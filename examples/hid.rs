// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::{
    entry,
    ports::srv::Srv,
    result::Result,
    services::hid::Hid,
    svc::{self, Timeout},
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

    let hid = Hid::init(&srv)?;

    info!("Press START to exit");
    while !hid.last_keypad().start() {
        svc::sleep_thread(Timeout::from_nanoseconds(16_666_667));
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
