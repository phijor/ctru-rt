// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use core::time::Duration;
use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::os::WeakHandle;
use ctru_rt::{
    debug::{init_log, SvcDebugLog},
    entry,
    result::Result,
    svc, thread,
};

use log::{error, info};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog::default();
    let _ = writeln!(log, "{}", info);

    svc::exit_thread()
}

fn run() -> Result<()> {
    info!("Hello from the main thread!");

    const NUM_THREADS: i32 = 3;
    let priority = svc::get_thread_priority(WeakHandle::active_thread())?;

    let threads = (1..=NUM_THREADS)
        .map(|id| {
            let timeout = Duration::from_secs(id as u64);
            let color = 30 + id.min(7);

            thread::ThreadBuilder::default()
                .with_priority(priority - id)
                .spawn(move || {
                    info!("\x1b[{}mThread {}\x1b[0m: Hello!", color, id);

                    for i in 0..5 {
                        info!("\x1b[{}mThread {}\x1b[0m: Counting {}...", color, id, 5 - i);
                        svc::sleep_thread(timeout.into());
                    }
                })
        })
        .collect::<Result<Vec<_>>>()?;

    info!("Waiting for {} threads to finish...", threads.len());

    for (i, thread) in threads.into_iter().enumerate() {
        if let Err(e) = thread.join() {
            error!("Failed to join thread {}: {:?}", i, e);
        }
    }

    Ok(())
}

#[entry]
fn main() {
    let _ = init_log();

    if let Err(e) = run() {
        error!("Failed to run thread example: {:?}", e);
    }
}
