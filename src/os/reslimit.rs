use core::marker::PhantomData;

use crate::{result::Result, svc};

use super::{BorrowHandle, Handle, WeakHandle};

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

pub struct Limit<'limits, 'proc> {
    type_: LimitType,
    limits: &'limits ProcessLimits<'proc>,
}

impl<'limits, 'proc> Limit<'limits, 'proc> {
    pub fn limit(&self) -> Result<i64> {
        let mut value = [0i64];
        svc::get_resource_limit_values(
            self.limits.handle.borrow_handle(),
            &mut value,
            &[self.type_],
        )?;

        Ok(value[0])
    }

    pub fn current(&self) -> Result<i64> {
        let mut value = [0i64];
        svc::get_resource_limit_current_values(
            self.limits.handle.borrow_handle(),
            &mut value,
            &[self.type_],
        )?;

        Ok(value[0])
    }

    pub fn remaining(&self) -> Result<i64> {
        let current = self.current()?;
        let limit = self.limit()?;

        Ok(limit - current)
    }
}

pub struct ProcessLimits<'proc> {
    handle: Handle,
    _process: PhantomData<&'proc Handle>,
}

impl<'proc> ProcessLimits<'proc> {
    pub(crate) fn get<'limits>(&'limits self, type_: LimitType) -> Limit<'limits, 'proc> {
        Limit {
            type_,
            limits: self,
        }
    }

    pub fn memory_allocatable(&self) -> Limit {
        self.get(LimitType::MemoryAllocatable)
    }
}

pub fn process_limits(process_handle: WeakHandle) -> Result<ProcessLimits> {
    let handle = svc::get_resource_limit(process_handle)?;

    Ok(ProcessLimits {
        handle,
        _process: PhantomData,
    })
}
