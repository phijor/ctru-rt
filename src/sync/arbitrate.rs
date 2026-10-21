// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::os::{AsHandle, OwnedHandle};
use crate::result::{Result, ERROR_ALREADY_INITIALIZED};
use crate::svc::{self, Timeout};
use crate::sync::AtomicHandle;

use core::sync::atomic::AtomicI32;

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
