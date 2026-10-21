// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::os::{AsHandle, OwnedHandle};
use crate::result::{Result, ERROR_ALREADY_INITIALIZED};
use crate::svc::{self, Timeout};
use crate::sync::AtomicHandle;

use core::sync::atomic::{AtomicI32, Ordering};

#[derive(Debug)]
#[repr(u32)]
#[non_exhaustive]
pub enum ArbitrationType {
    Signal = 0,
    WaitIfLessThan = 1,
    DecrementAndWaitIfLessThan = 2,
    WaitIfLessThanTimeout = 3,
    DecrementAndWaitIfLessThanTimeout = 4,
}

impl svc::IntoRegister for ArbitrationType {
    type Register = u32;
    unsafe fn into_register(self) -> Self::Register {
        self as Self::Register
    }
}

#[derive(Debug)]
pub(crate) struct AddressArbiter {
    arbiter: AtomicHandle,
}

impl AddressArbiter {
    pub const fn new_closed() -> Self {
        Self {
            arbiter: AtomicHandle::new_closed(),
        }
    }

    pub fn init(&self) -> Result<()> {
        // Return an error if the handle is already open,
        // before asking the kernel for a new one.
        if self.arbiter.get().is_open() {
            return Err(ERROR_ALREADY_INITIALIZED);
        }

        // Ask the kernel to create a new address arbiter
        let arbiter: OwnedHandle = svc::create_address_arbiter()?;

        // Take ownership of the arbiter handle.
        // If the arbiter has already been initialized, the new handle is dropped (and closed).
        self.arbiter
            .try_set(arbiter)
            .map(|_| ())
            .map_err(|_| ERROR_ALREADY_INITIALIZED)
    }

    fn arbitrate(
        &self,
        word: &AtomicI32,
        arbitration_type: ArbitrationType,
        value: i32,
        timeout: Timeout,
    ) -> Result<()> {
        svc::arbitrate_address(
            self.arbiter.as_handle(),
            word.as_ptr() as usize,
            arbitration_type,
            value,
            timeout,
        )
    }

    pub fn wake_up(&self, word: &AtomicI32, num_waiters: usize, timeout: Timeout) -> Result<()> {
        let num_waiters: i32 = if num_waiters <= i32::MAX as usize {
            num_waiters as i32
        } else {
            panic!("Cannot wake up more than i32::MAX waiters")
        };

        self.arbitrate(word, ArbitrationType::Signal, num_waiters, timeout)
    }

    pub fn wake_up_all(&self, word: &AtomicI32, timeout: Timeout) -> Result<()> {
        self.arbitrate(word, ArbitrationType::Signal, -1, timeout)
    }

    pub fn wait_if_less_than(&self, word: &AtomicI32, value: i32) -> Result<()> {
        self.arbitrate(
            word,
            ArbitrationType::WaitIfLessThan,
            value,
            Timeout::none(),
        )
    }
}

pub(crate) static ADDRESS_ARBITER: AddressArbiter = AddressArbiter::new_closed();

pub(crate) fn init() -> Result<()> {
    ADDRESS_ARBITER.init()
}

#[derive(Debug)]
pub struct Semaphore {
    available: AtomicI32,
}

impl Semaphore {
    pub const fn new(count: usize) -> Self {
        if count > i32::MAX as usize {
            panic!("Cannot initialize Semaphore with count exceeding i32::MAX")
        }

        Self {
            available: AtomicI32::new(count as i32),
        }
    }

    /// Check whether at least one resource is available.
    #[inline]
    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Relaxed) >= 1
    }

    /// Attempt to acquire one resource.
    ///
    /// Returns [`true`] if a resource was successfully acquired, [`false`] otherwise.
    pub fn try_acquire(&self) -> bool {
        // Check how many copies of the resource are currently available.
        //
        // Relaxed load: This load does not have to synchronize with any write:
        // - If no resource is available, `self.available` will not get updated.
        // - If at least one resource is available, `self.available` will get
        //  reloaded with Acquire semantics in a CAS-loop.  It is that read which
        //  will have to synchronize.
        let mut currently_available = self.available.load(Ordering::Relaxed);

        loop {
            // If no copies are available, do not acquire one and bail.
            if currently_available <= 0 {
                return false;
            }

            // Attempt to take one copy.
            // If the count has changed since the initial load, try again.
            match self.available.compare_exchange_weak(
                currently_available,
                currently_available - 1,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(updated_available) => currently_available = updated_available,
            }
        }
    }

    /// Acquire one resource, wait if necessary.
    ///
    /// Returns [`Ok`] if the resource was acquired successfully,
    /// and [`Err`] if waiting for the resource failed.
    pub fn acquire(&self) -> Result<()> {
        // Try to acquire one resource.
        // If this fails, wait until at least one is available, then try again.
        while !self.try_acquire() {
            // Ensure the global address arbiter has been initialized.
            debug_assert!(
                ADDRESS_ARBITER.arbiter.is_open(),
                "Global address arbiter has not been initialized"
            );

            // Ask the kernel to park this thread, giving other threads a chance to make progress.
            ADDRESS_ARBITER.wait_if_less_than(&self.available, 1)?;
        }

        Ok(())
    }

    /// Release one resource.
    ///
    /// Returns [`Err`] if the kernel failed to notify other waiting threads that an extra resource
    /// is available again.  The resource is released successfully *regardless*.
    pub fn release(&self) -> Result<()> {
        // Return one resource.  This store synchronizes with the successful CAS in `try_acquire`.
        let _ = self.available.fetch_add(1, Ordering::Release);

        // Ask the kernel to wake up at most one thread waiting to acquire a resource.
        ADDRESS_ARBITER.wake_up(&self.available, 1, Timeout::none())
    }
}

pub struct RawMutex {
    resource: Semaphore,
}

unsafe impl lock_api::RawMutex for RawMutex {
    const INIT: Self = Self {
        resource: Semaphore::new(1),
    };

    type GuardMarker = lock_api::GuardSend;

    fn lock(&self) {
        self.resource
            .acquire()
            .expect("Failed to acquire resource from arbitrated semaphore");
    }

    fn try_lock(&self) -> bool {
        self.resource.try_acquire()
    }

    unsafe fn unlock(&self) {
        // Release outside of `debug_assert!`: Its argument is not evaluated in release builds.
        let released = self.resource.release();
        debug_assert!(
            released.is_ok(),
            "Failed to release resource to arbitrated semaphore"
        )
    }

    fn is_locked(&self) -> bool {
        !self.resource.is_available()
    }
}

pub type Mutex<T> = lock_api::Mutex<RawMutex, T>;
pub type MutexGuard<'a, T> = lock_api::MutexGuard<'a, RawMutex, T>;
