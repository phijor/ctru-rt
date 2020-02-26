use crate::{result::Result, svc};

use volatile::ReadOnly;

pub mod cfgmem;

#[derive(Debug)]
#[repr(transparent)]
pub struct Handle(u32);

impl !Send for Handle {}

impl Handle {
    pub(crate) const fn new(raw_handle: u32) -> Self {
        Self(raw_handle)
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

    pub(crate) fn copy_raw(&self) -> Self {
        Self(self.0)
    }

    pub(crate) fn invalid() -> Self {
        Self(0)
    }

    pub fn try_clone(&self) -> Result<Self> {
        svc::duplicate_handle(self)
    }
}

impl Clone for Handle {
    fn clone(&self) -> Self {
        self.try_clone()
            .expect("system call duplicating handle failed")
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
