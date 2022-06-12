// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::{result::Result, svc};

use core::{fmt, marker::PhantomData, num::NonZeroU32};

use log::debug;

pub mod cfgmem;
pub mod mem;
pub mod reslimit;
pub mod sharedmem;

pub type RawHandle = u32;
pub type ValidRawHandle = NonZeroU32;

#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct BorrowedHandle<'handle> {
    handle: RawHandle,
    _owner: PhantomData<&'handle OwnedHandle>,
}

pub(crate) const CLOSED_HANDLE: RawHandle = 0;

impl BorrowedHandle<'_> {
    pub(crate) const fn new(raw_handle: RawHandle) -> Self {
        Self {
            handle: raw_handle,
            _owner: PhantomData,
        }
    }

    pub const fn active_thread() -> Self {
        Self::new(0xFFFF_8000)
    }

    pub const fn active_process() -> Self {
        Self::new(0xFFFF_8001)
    }

    pub(crate) fn as_raw(&self) -> RawHandle {
        self.handle
    }

    pub(crate) fn into_raw(self) -> RawHandle {
        self.handle
    }

    pub(crate) const fn invalid() -> Self {
        Self::new(CLOSED_HANDLE)
    }
}

#[repr(transparent)]
pub struct OwnedHandle {
    handle: ValidRawHandle,
    // _unsend_marker: PhantomData<*const u32>,
}

impl OwnedHandle {
    pub unsafe fn new(raw_handle: RawHandle) -> Option<Self> {
        let handle = ValidRawHandle::new(raw_handle)?;
        Some(Self { handle })
    }

    pub fn close(&mut self) -> Result<()> {
        let raw_handle: RawHandle = self.handle.get();
        debug_assert_ne!(
            raw_handle, CLOSED_HANDLE,
            "Calling OwnedHandle::close() on an invalid Handle!"
        );
        unsafe { svc::close_handle(raw_handle) }
    }

    pub fn handle(&self) -> BorrowedHandle {
        BorrowedHandle::new(self.handle.get())
    }

    pub fn try_duplicate(&self) -> Result<Self> {
        svc::duplicate_handle(self.handle())
    }

    pub const fn leak(self) -> RawHandle {
        let raw_handle = self.handle.get();

        // Do not run the destructor for this handle.
        // This way, svc::close_handle() is *not* called on the contained RawHandle.
        core::mem::forget(self);

        raw_handle
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        debug!("Dropping handle {:08x?}", self);
        let _ = self.close();
    }
}

impl fmt::Debug for OwnedHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Handle").field(&self.handle).finish()
    }
}

impl super::svc::FromRegister for OwnedHandle {
    unsafe fn from_register(reg: u32) -> Self {
        OwnedHandle::new(reg).expect("Register contained an invalid Handle")
    }
}

impl super::svc::IntoRegister for OwnedHandle {
    type Register = u32;
    unsafe fn into_register(self) -> u32 {
        self.leak()
    }
}

impl super::svc::IntoRegister for BorrowedHandle<'_> {
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
        let cfgmem_ptr = match self {
            MemoryRegion::All => {
                return MemoryRegion::Application.size()
                    + MemoryRegion::System.size()
                    + MemoryRegion::Base.size()
            }
            MemoryRegion::Application => cfgmem::APPMEMALLOC,
            MemoryRegion::System => cfgmem::SYSMEMALLOC,
            MemoryRegion::Base => cfgmem::BASEMEMALLOC,
        };

        cfgmem_ptr.read()
    }

    pub fn used(&self) -> Result<u64> {
        const MEM_USED: u32 = 0;
        unsafe { svc::get_system_info(MEM_USED, *self as i32).map(|val| val as u64) }
    }
}

pub trait BorrowHandle {
    fn borrow_handle(&self) -> BorrowedHandle;
}

impl BorrowHandle for OwnedHandle {
    fn borrow_handle(&self) -> BorrowedHandle {
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
