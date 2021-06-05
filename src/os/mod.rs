use crate::{result::Result, svc};

use core::{fmt, marker::PhantomData, num::NonZeroU32};

use log::debug;
use volatile::ReadOnly;

pub mod cfgmem;
pub mod mem;
pub mod reslimit;
pub mod sharedmem;

#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct WeakHandle<'a>(u32, PhantomData<&'a u32>);

pub(crate) const CLOSED_HANDLE: u32 = 0;

impl WeakHandle<'_> {
    pub(crate) const fn new(raw_handle: u32) -> Self {
        Self(raw_handle, PhantomData)
    }

    pub const fn active_thread() -> Self {
        Self::new(0xFFFF_8000)
    }

    pub const fn active_process() -> Self {
        Self::new(0xFFFF_8001)
    }

    pub(crate) fn as_raw(&self) -> u32 {
        self.0
    }

    pub(crate) fn into_raw(self) -> u32 {
        self.0
    }

    pub(crate) const fn invalid() -> Self {
        Self::new(CLOSED_HANDLE)
    }
}

#[repr(transparent)]
pub struct Handle {
    handle: Option<NonZeroU32>,
    // _unsend_marker: PhantomData<*const u32>,
}

impl Handle {
    pub unsafe fn new(raw_handle: u32) -> Self {
        Self {
            handle: NonZeroU32::new(raw_handle),
            // _unsend_marker: PhantomData,
        }
    }

    pub unsafe fn own(handle: WeakHandle) -> Self {
        Self::new(handle.as_raw())
    }

    pub const fn new_closed() -> Self {
        Self {
            handle: None,
            // _unsend_marker: PhantomData,
        }
    }

    pub fn take(&mut self) -> Self {
        Self {
            handle: self.handle.take(),
            // _unsend_marker: PhantomData,
        }
    }

    pub fn close(&mut self) -> Result<()> {
        match self.handle {
            Some(_) => {
                let result = svc::close_handle(self.borrow_handle());
                self.handle = None;
                result
            }
            None => Ok(()),
        }
    }

    pub fn handle(&self) -> WeakHandle {
        match self.handle {
            None => WeakHandle::invalid(),
            Some(h) => WeakHandle::new(h.into()),
        }
    }

    pub fn try_duplicate(&self) -> Result<Self> {
        svc::duplicate_handle(self.handle())
    }

    pub const fn leak(self) -> u32 {
        let raw_handle = self.handle;
        core::mem::forget(self);

        match raw_handle {
            Some(h) => h.get(),
            None => CLOSED_HANDLE,
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        debug!("Dropping handle {:08x?}", self);
        let _ = self.close();
    }
}

impl Default for Handle {
    fn default() -> Self {
        Self::new_closed()
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.handle {
            Some(h) => f.debug_tuple("Handle").field(&h).finish(),
            None => f.write_str("Handle::invalid()"),
        }
    }
}

impl super::svc::FromRegister for Handle {
    unsafe fn from_register(reg: u32) -> Self {
        Self::new(reg)
    }
}

impl super::svc::IntoRegister for Handle {
    type Register = u32;
    unsafe fn into_register(self) -> u32 {
        self.leak()
    }
}

impl super::svc::IntoRegister for WeakHandle<'_> {
    type Register = u32;
    unsafe fn into_register(self) -> u32 {
        self.into_raw()
    }
}

#[derive(Debug, Copy, Clone)]
pub enum MemoryRegion {
    All = 0,
    Application = 1,
    System = 2,
    Base = 3,
}

impl MemoryRegion {
    pub fn size(&self) -> usize {
        let cfgmem_ptr: *const ReadOnly<usize> = match self {
            MemoryRegion::All => {
                return MemoryRegion::Application.size()
                    + MemoryRegion::System.size()
                    + MemoryRegion::Base.size()
            }
            MemoryRegion::Application => cfgmem::APPMEMALLOC,
            MemoryRegion::System => cfgmem::SYSMEMALLOC,
            MemoryRegion::Base => cfgmem::BASEMEMALLOC,
        };

        unsafe { cfgmem_ptr.read() }.read()
    }

    pub fn used(&self) -> Result<u64> {
        const MEM_USED: u32 = 0;
        unsafe { svc::get_system_info(MEM_USED, *self as i32).map(|val| val as u64) }
    }
}

pub trait BorrowHandle {
    fn borrow_handle<'a>(&'a self) -> WeakHandle<'a>;
}

impl BorrowHandle for Handle {
    fn borrow_handle(&self) -> WeakHandle {
        self.handle()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SystemTick(u64);

impl SystemTick {
    pub fn new(ticks: u64) -> Self {
        Self(ticks)
    }

    pub fn now() -> Self {
        Self(svc::get_system_tick_count())
    }

    pub const fn count(&self) -> u64 {
        self.0
    }
}