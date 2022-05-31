// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::{collections::BTreeMap, vec::Vec};
use ctru_rt::svc::Timeout;

use core::time::Duration;
use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::os::WeakHandle;
use ctru_rt::{
    debug::{init_log, SvcDebugLog},
    entry,
    result::Result,
    svc, sync, thread,
};

use log::{error, info, warn};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog::default();
    let _ = writeln!(log, "{}", info);

    svc::exit_thread()
}

/// Spawn a bunch of worker threads which slowly count to five and return some value when done.
fn run() -> Result<()> {
    info!("Hello from the main thread!");

    const NUM_THREADS: usize = 5;

    // An event for each worker thread, used to signal to the main thread that the worker thread done.
    let done_events: Vec<sync::Event> = (0..NUM_THREADS)
        .map(|_| sync::Event::new(sync::ResetType::OneShot))
        .collect::<Result<_>>()?;

    // Spawn the threads and collect a map of { thread ID → JoinHandle for that thread }.
    let mut threads: BTreeMap<usize, _> = done_events
        .iter()
        .zip(0usize..NUM_THREADS)
        .map(|(event, id)| {
            let timeout = Duration::from_secs((NUM_THREADS - id) as u64);
            let color = 30 + id.min(7);

            let event = event.duplicate()?;

            let thread = thread::spawn(move || {
                // This closure is executed in a new thread.

                info!("\x1b[{}mThread {}\x1b[0m: Hello!", color, id);

                // Slowly count to five
                for i in 0..5 {
                    info!("\x1b[{}mThread {}\x1b[0m: Counting {}...", color, id, 5 - i);
                    svc::sleep_thread(timeout.into());
                }

                // Signal the main thread that we're done here.
                // The main thread sleeps while waiting for these events to occur.
                let _ = event.signal();

                // Return this thread's ID.
                return id;
            })?;
            Ok((id, thread))
        })
        .collect::<Result<_>>()?;

    info!("Waiting for {} threads to finish...", threads.len());

    let mut sanity = 0;
    while !threads.is_empty() && sanity <= NUM_THREADS {
        let signaled_idx = sync::Event::wait_any(done_events.as_slice(), Timeout::forever())?;
        sanity += 1;
        info!("Got signal {}", signaled_idx);
        done_events.get(signaled_idx).map(|ev| ev.clear());

        let thread = if let Some(thread) = threads.remove(&signaled_idx) {
            thread
        } else {
            warn!("Event {} signaled again!", signaled_idx);
            continue;
        };

        match thread.join() {
            Ok(val) => {
                info!("Thread {} returned {}.", signaled_idx, val);
            }
            Err(e) => {
                error!("Failed to join thread {}: {:?}", signaled_idx, e);
            }
        }
    }

    if !threads.is_empty() {
        for (idx, thread) in threads.into_iter() {
            match thread.join() {
                Ok(val) => {
                    info!("Lingering thread {} returned {}.", idx, val);
                }
                Err(e) => {
                    error!("Failed to join lingering thread {}: {:?}", idx, e);
                }
            }
        }
    } else {
        info!("All threads finished!");
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
