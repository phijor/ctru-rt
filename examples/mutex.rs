// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]
#![allow(clippy::default_constructed_unit_structs)]

extern crate alloc;

use ctru_rt::svc::Timeout;

use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::{
    debug::{init_log, SvcDebugLog},
    entry,
    result::Result,
    svc,
    sync::Mutex,
};

use log::{error, info};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog::default();
    let _ = writeln!(log, "{}", info);

    svc::exit_thread()
}

/// Spawn a bunch of worker threads which slowly count to five and return some value when done.
fn run() -> Result<()> {
    let mtx = Mutex::new(0u32);

    loop {
        let mut data = mtx.lock();
        let val: u32 = data.wrapping_add(1);
        info!("Value: {}", val);
        *data = val;

        svc::sleep_thread(Timeout::from_seconds(1));
    }
}

#[entry]
fn main() {
    let _ = init_log();

    if let Err(e) = run() {
        error!("Failed to run thread example: {:?}", e);
    }
}
