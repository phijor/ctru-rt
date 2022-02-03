// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::time::Duration;
use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::{
    debug::{init_log, SvcDebugLog},
    entry, env, os,
    result::ResultCode,
};

use log::info;

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog;
    let _ = write!(log, "{}", info);

    ctru_rt::svc::exit_process()
}

#[entry]
fn main() {
    let _ = init_log().expect("Failed to initialize logger");

    let app_id = env::app_id();
    let app_mem_used = os::MemoryRegion::Application
        .used()
        .expect("Failed to get memory information");
    info!(
        "Hello, World!, app_id is {:#0x}, app mem used: {:#0x}",
        app_id, app_mem_used
    );

    let rc = ResultCode::from(0x2a07);
    info!("Some result code: {:?}", rc);

    let _ = ctru_rt::svc::sleep_thread(Duration::from_secs(2).into());

    info!("Bye-bye!");
}
