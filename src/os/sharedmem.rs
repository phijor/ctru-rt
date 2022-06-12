// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use super::{
    mem::{MemoryPermission, MemoryState, QueryResult},
    OwnedHandle,
};
use crate::result::{Result, ERROR_OUT_OF_MEMORY};
use crate::svc;

use log::debug;

#[derive(Debug)]
#[must_use = "Dropping a shared memory block without unmapping it leaks the shared memory handle"]
pub struct MappedBlock {
    start: usize,
    size: usize,
    handle: OwnedHandle,
}

impl MappedBlock {
    pub fn size(&self) -> usize {
        self.size
    }

    pub fn as_ptr(&self) -> *const u32 {
        self.start as *const u32
    }

    pub fn as_mut_ptr(&mut self) -> *mut u32 {
        self.start as *mut u32
    }

    pub fn as_slice(&self) -> &[AtomicU32] {
        unsafe {
            core::slice::from_raw_parts(
                self.start as *const AtomicU32,
                self.size / core::mem::size_of::<u32>(),
            )
        }
    }

    pub unsafe fn as_mut_slice_raw(&mut self) -> &mut [u32] {
        core::slice::from_raw_parts_mut(
            self.start as *mut u32,
            self.size / core::mem::size_of::<u32>(),
        )
    }
}

#[derive(Debug)]
pub struct SharedMemoryMapper {
    next_candidate: AtomicUsize,
}

const SHAREDMEM_START: usize = 0x1000_0000;
const SHAREDMEM_END: usize = 0x1400_0000;

static mut GLOBAL_SHAREDMEMORY_MAPPER: SharedMemoryMapper = SharedMemoryMapper::new();

impl SharedMemoryMapper {
    pub const fn new() -> Self {
        Self {
            next_candidate: AtomicUsize::new(SHAREDMEM_START),
        }
    }

    pub(crate) fn global() -> &'static Self {
        // (UN)SAFETY: I *know* global statics are bad.
        //
        // This will get a proper implementation once there's support for `RwLock`s.
        unsafe { &GLOBAL_SHAREDMEMORY_MAPPER }
    }

    pub fn map(
        &self,
        memory_handle: OwnedHandle,
        size: usize,
        my_permissions: MemoryPermission,
        other_permissions: MemoryPermission,
    ) -> Result<MappedBlock> {
        let size = (size + 0xFFF) & !0xFFF;

        let candidate = self.next_candidate.load(Ordering::Acquire);
        let address = Self::find_gap(candidate, size)?.ok_or(ERROR_OUT_OF_MEMORY)?;

        let next = candidate.saturating_add(size).min(SHAREDMEM_END);
        self.next_candidate.store(next, Ordering::Release);

        debug!(
            "Mapping memory block at {:p}, size = 0x{:x}, handle = {:?}",
            address as *const u32, size, memory_handle,
        );

        // TODO: figure out how/when this is sound.
        // Probably never at the moment; we need to tie the lifetime of the mapped block to the
        // lifetime of `memory_handle`.
        unsafe {
            svc::map_memory_block(
                memory_handle.handle(),
                address,
                my_permissions,
                other_permissions,
            )?
        };

        Ok(MappedBlock {
            start: address,
            size,
            handle: memory_handle,
        })
    }

    pub fn unmap(&self, block: MappedBlock) -> Result<OwnedHandle> {
        unsafe { svc::unmap_memory_block(block.handle.handle(), block.start as usize)? }

        self.next_candidate.store(block.start, Ordering::Release);

        Ok(block.handle)
    }

    fn remaining_free(block: &QueryResult, address: usize) -> Option<usize> {
        let offset_in_block = address.checked_sub(block.base_process_virtual_address)?;
        block.size.checked_sub(offset_in_block)
    }

    fn find_gap_within(start: usize, end: usize, size: usize) -> Result<Option<usize>> {
        let mut candidate_addr = start;

        while candidate_addr
            .checked_add(size)
            .map(|chunk_end| chunk_end < end)
            .unwrap_or(false)
        {
            let block: QueryResult = unsafe { svc::query_memory(candidate_addr)? };

            if let MemoryState::Free = block.state {
                if let Some(free) = Self::remaining_free(&block, candidate_addr) {
                    if free >= size {
                        return Ok(Some(candidate_addr));
                    }
                }
            };

            match candidate_addr.checked_add(block.size) {
                Some(next) => candidate_addr = next,
                None => return Ok(None),
            }
        }

        Ok(None)
    }

    fn find_gap(candidate: usize, size: usize) -> Result<Option<usize>> {
        match Self::find_gap_within(candidate, SHAREDMEM_END, size)? {
            Some(address) => Ok(Some(address)),
            None => Self::find_gap_within(SHAREDMEM_START, candidate, size),
        }
    }
}
