// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::num::NonZeroUsize;

use log::debug;
use num_enum::IntoPrimitive;

use crate::{heap::PageBuffer, result::Result, svc};

use super::{AsHandle, BorrowedHandle, MemoryRegion, OwnedHandle};

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum MemoryOperationTarget {
    Heap = 0x0_0000,
    Linear = 0x1_0000,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum MemoryOperationRegion {
    App = (MemoryRegion::Application as u32) << 16,
    System = (MemoryRegion::System as u32) << 16,
    Base = (MemoryRegion::Base as u32) << 16,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum MemoryOperationAction {
    Free = 1,
    Reserve = 2,
    Allocate = 3,
    Map = 4,
    Unmap = 5,
    ChangeProtection = 6,
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryOperation(pub(crate) u32);

impl MemoryOperation {
    #[inline]
    pub const fn new(
        action: MemoryOperationAction,
        region: MemoryOperationRegion,
        target: MemoryOperationTarget,
    ) -> Self {
        Self((action as u32) | (region as u32) | (target as u32))
    }

    #[inline]
    pub const fn allocate() -> Self {
        Self(MemoryOperationAction::Allocate as u32)
    }

    #[inline]
    pub const fn linear(self) -> Self {
        Self(self.0 | MemoryOperationTarget::Linear as u32)
    }
}

#[derive(Debug, Clone, Copy, IntoPrimitive)]
#[repr(u32)]
pub enum MemoryPermission {
    None = 0,
    R = 1,
    W = 2,
    Rw = 3,
    X = 4,
    Rx = 5,
    Wx = 6,
    Rwx = 7,
    DontCare = 0x1000_0000,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum MemoryState {
    Free = 0,
    Reserved = 1,
    Io = 2,
    Static = 3,
    Code = 4,
    Private = 5,
    Shared = 6,
    Continuous = 7,
    Aliased = 8,
    Alias = 9,
    AliasCode = 10,
    Locked = 11,
}

#[derive(Debug)]
pub struct QueryResult {
    pub base_process_virtual_address: usize,
    pub size: usize,
    pub permission: MemoryPermission,
    pub state: MemoryState,
    pub page_flags: u32,
}

#[derive(Debug)]
pub(crate) struct MemoryBlock {
    handle: OwnedHandle,
    buffer: PageBuffer,
}

impl MemoryBlock {
    pub fn new(num_pages: NonZeroUsize) -> Result<Self> {
        let mut buffer = PageBuffer::allocate_pages(num_pages).expect("Context allocation failed");

        debug!("Creating memory block for buffer {:?}", buffer);
        let handle = unsafe {
            svc::create_memory_block(
                buffer.as_mut_ptr(),
                MemoryPermission::None,
                MemoryPermission::Rw,
            )?
        };

        debug!("Got buffer handle: {:08x?}", handle);
        Ok(MemoryBlock { handle, buffer })
    }

    pub fn size(&self) -> usize {
        self.buffer.size()
    }
}

impl AsHandle for MemoryBlock {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}
