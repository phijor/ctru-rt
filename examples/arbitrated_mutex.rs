// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stress test for [`ctru_rt::sync::arbitrate::Mutex`].
//!
//! 1. *Counter*: Worker threads at different priorities increment a shared counter.  Each worker
//!    reads the counter, sleeps while still holding the lock, then writes the incremented value.
//!    If mutual exclusion were broken, increments would get lost.
//! 2. *Priority*: A low-priority thread holds the lock while sleeping, and a high-priority thread
//!    tries to take it.  The high-priority thread has to block in the kernel (instead of spinning
//!    and starving the holder) and acquire the lock once it is released.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::{
    panic::PanicInfo,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};

use ctru_rt::{
    debug::init_log,
    entry,
    os::{AsHandle, PseudoHandle},
    result::Result,
    svc::{self, Timeout},
    sync::{self, arbitrate::Mutex},
    thread,
};

use log::{error, info};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    ctru_rt::panic::user_break(info)
}

const NUM_WORKERS: u32 = 4;
const ITERATIONS: u32 = 50;

/// The counter shared by all workers in the counter test.
static COUNTER: Mutex<u32> = Mutex::new(0);

/// Number of times a worker found the lock taken and had to wait for it.
static CONTENDED: AtomicU32 = AtomicU32::new(0);

/// Lock used by the priority test.
static PRIORITY_LOCK: Mutex<()> = Mutex::new(());

/// Set by the low-priority thread right before it releases [`PRIORITY_LOCK`].
static RELEASED: AtomicBool = AtomicBool::new(false);

fn main_priority() -> i32 {
    svc::get_thread_priority(PseudoHandle::ActiveThread.as_handle()).unwrap_or(0x30)
}

/// Lock the counter, recording whether the lock was contended.
fn lock_counter() -> sync::arbitrate::MutexGuard<'static, u32> {
    match COUNTER.try_lock() {
        Some(guard) => guard,
        None => {
            CONTENDED.fetch_add(1, Ordering::Relaxed);
            COUNTER.lock()
        }
    }
}

fn counter_test() -> Result<bool> {
    let base = main_priority();

    // Spread the workers around the main thread's priority: some run above it, some below.
    let workers: Vec<_> = (0..NUM_WORKERS)
        .map(|id| {
            let priority = base - 2 + id as i32;

            thread::ThreadBuilder::default()
                .with_priority(priority)
                .spawn(move || {
                    for _ in 0..ITERATIONS {
                        let mut counter = lock_counter();

                        // Read, give other threads a chance to run while holding the lock, write.
                        let value = *counter;
                        svc::sleep_thread(Duration::from_micros(100));
                        *counter = value + 1;

                        drop(counter);
                        svc::sleep_thread(Duration::from_micros(50));
                    }

                    info!("Counter worker {} (priority {}) done", id, priority);
                })
        })
        .collect::<Result<_>>()?;

    for worker in workers {
        worker.join()?;
    }

    let expected = NUM_WORKERS * ITERATIONS;
    let actual = *COUNTER.lock();
    let contended = CONTENDED.load(Ordering::Relaxed);

    info!(
        "Counter test: expected {}, got {}, lock contended {} times",
        expected, actual, contended
    );

    if contended == 0 {
        info!("Counter test never hit the contended path; the kernel wait was not exercised");
    }

    Ok(actual == expected)
}

fn priority_test() -> Result<bool> {
    let base = main_priority();

    // Signaled by the low-priority thread once it holds the lock.
    let locked = sync::Event::new(sync::ResetType::OneShot)?;
    let locked_signal = locked.duplicate()?;

    let low = thread::ThreadBuilder::default()
        .with_priority(base + 2)
        .spawn(move || {
            let guard = PRIORITY_LOCK.lock();
            info!("Priority test: low-priority thread holds the lock");
            let _ = locked_signal.signal();

            // Hold the lock long enough for the high-priority thread to start waiting on it.
            svc::sleep_thread(Duration::from_secs(1));

            RELEASED.store(true, Ordering::Relaxed);
            info!("Priority test: low-priority thread releases the lock");
            drop(guard);
        })?;

    // Wait until the low-priority thread actually holds the lock.
    locked.wait(Timeout::from_seconds(5))?;

    let high = thread::ThreadBuilder::default()
        .with_priority(base - 2)
        .spawn(|| {
            info!("Priority test: high-priority thread tries to lock");
            let _guard = PRIORITY_LOCK.lock();

            // The lock's Acquire/Release ordering makes the store before the unlock visible here.
            let released = RELEASED.load(Ordering::Relaxed);
            info!(
                "Priority test: high-priority thread got the lock (released before: {})",
                released
            );
            released
        })?;

    let got_lock_after_release = high.join()?.unwrap();
    low.join()?;

    Ok(got_lock_after_release)
}

fn run() -> Result<()> {
    info!("Starting arbitrated mutex stress test");

    let counter_ok = counter_test()?;
    let priority_ok = priority_test()?;

    match (counter_ok, priority_ok) {
        (true, true) => info!("All tests passed!"),
        _ => error!(
            "Tests failed: counter {}, priority {}",
            if counter_ok { "ok" } else { "FAILED" },
            if priority_ok { "ok" } else { "FAILED" },
        ),
    }

    Ok(())
}

#[entry]
fn main() {
    let _ = init_log();

    if let Err(e) = run() {
        error!("Failed to run arbitrated mutex stress test: {:?}", e);
    }
}
