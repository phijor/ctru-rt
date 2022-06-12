// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo, time::Duration};

use log::{error, info};

use ctru_rt::{
    debug::SvcDebugLog,
    entry,
    heap::{PageAlignError, PageAlignedBuffer},
    ports::srv::Srv,
    result::{ErrorCode, Level, Module, Result, Summary},
    services::soc::{Domain, Protocol, Soc, Type},
    svc::{sleep_thread, UserBreakReason},
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog::default();
    let _ = writeln!(log, "[PANIC] {}", info);

    ctru_rt::svc::user_break(UserBreakReason::Panic)
}

fn run() -> Result<()> {
    let srv = Srv::init()?;

    let _ = info!("Initialized srv: {:#0x?}", srv);

    let buffer = PageAlignedBuffer::allocate(0x4000).map_err(|e| match e {
        PageAlignError::Alloc => {
            ErrorCode::new(Level::Fatal, Summary::OutOfResource, Module::Os, 1011)
        }
        PageAlignError::Layout(_) => {
            ErrorCode::new(Level::Fatal, Summary::OutOfResource, Module::Os, 1009)
        }
    })?;
    let soc = Soc::init(&srv, buffer)?;
    let _ = info!("Initialized soc: {:#0x?}", soc);

    let socket = soc.socket(Domain::AfInet, Type::Stream, Protocol::Default)?;
    let _ = info!("Initialized socket: {:0x?}", socket);

    info!("gethostid() = {:03?}", soc.gethostid());

    loop {
        info!("Done...");
        sleep_thread(Duration::from_secs(1).into());
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
