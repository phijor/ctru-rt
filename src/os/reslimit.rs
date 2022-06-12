// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::marker::PhantomData;

use crate::{result::Result, svc};

use super::{AsHandle, OwnedHandle, BorrowedHandle};

// /// Types of resource limit
// typedef enum {
// 	RESLIMIT_PRIORITY       = 0,        ///< Thread priority
// 	RESLIMIT_COMMIT         = 1,        ///< Quantity of allocatable memory
// 	RESLIMIT_THREAD         = 2,        ///< Number of threads
// 	RESLIMIT_EVENT          = 3,        ///< Number of events
// 	RESLIMIT_MUTEX          = 4,        ///< Number of mutexes
// 	RESLIMIT_SEMAPHORE      = 5,        ///< Number of semaphores
// 	RESLIMIT_TIMER          = 6,        ///< Number of timers
// 	RESLIMIT_SHAREDMEMORY   = 7,        ///< Number of shared memory objects, see @ref svcCreateMemoryBlock
// 	RESLIMIT_ADDRESSARBITER = 8,        ///< Number of address arbiters
// 	RESLIMIT_CPUTIME        = 9,        ///< CPU time. Value expressed in percentage regular until it reaches 90.
//
// 	RESLIMIT_BIT            = BIT(31),  ///< Forces enum size to be 32 bits
//
// } ResourceLimitType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LimitType {
    Priority = 0,
    MemoryAllocatable,
    Threads,
    Events,
    Mutexes,
    Semaphores,
    Timers,
    SharedMemoryHandles,
    AddressArbiters,
    CpuTime,
}

pub struct Limit<'limits> {
    type_: LimitType,
    limits_handle: BorrowedHandle<'limits>,
}

impl<'limits> Limit<'limits> {
    pub fn limit(&self) -> Result<i64> {
        let mut value = [0i64];
        svc::get_resource_limit_values(self.limits_handle, &mut value, &[self.type_])?;

        Ok(value[0])
    }

    pub fn current(&self) -> Result<i64> {
        let mut value = [0i64];
        svc::get_resource_limit_current_values(self.limits_handle, &mut value, &[self.type_])?;

        Ok(value[0])
    }

    pub fn remaining(&self) -> Result<i64> {
        let current = self.current()?;
        let limit = self.limit()?;

        Ok(limit - current)
    }
}

pub struct ProcessLimits<'proc> {
    handle: OwnedHandle,
    _process: PhantomData<&'proc OwnedHandle>,
}

impl<'proc> ProcessLimits<'proc> {
    pub(crate) fn get(&self, type_: LimitType) -> Limit<'_> {
        Limit {
            type_,
            limits_handle: self.handle.as_handle(),
        }
    }

    pub fn memory_allocatable(&self) -> Limit {
        self.get(LimitType::MemoryAllocatable)
    }
}

pub fn process_limits(process_handle: BorrowedHandle<'_>) -> Result<ProcessLimits<'_>> {
    let handle = svc::get_resource_limit(process_handle)?;

    Ok(ProcessLimits {
        handle,
        _process: PhantomData,
    })
}
