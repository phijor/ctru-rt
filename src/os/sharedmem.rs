use super::{
    mem::{MemoryPermission, MemoryState, QueryResult},
    Handle,
};
use crate::result::{ErrorCode, Level, Module, Result, Summary};
use crate::svc;

use log::debug;

#[derive(Debug)]
#[must_use = "Dropping a shared memory block without unmapping it leaks the shared memory handle"]
pub struct MappedBlock {
    start: usize,
    size: usize,
    handle: Handle,
}

impl MappedBlock {
    pub fn as_slice(&self) -> &[u32] {
        unsafe { core::slice::from_raw_parts(self.start as *const u32, self.size) }
    }
}

#[derive(Debug)]
pub struct SharedMemoryMapper {
    next_candidate: usize,
}

const SHAREDMEM_START: usize = 0x1000_0000;
const SHAREDMEM_END: usize = 0x1400_0000;

const ERROR_DESC_OUT_OF_MEMORY: u32 = 1011;

impl SharedMemoryMapper {
    pub fn new() -> Self {
        Self {
            next_candidate: SHAREDMEM_START,
        }
    }

    pub fn map(&mut self, memory_handle: Handle, size: usize) -> Result<MappedBlock> {
        let size = (size + 0xFFF) & !0xFFF;

        let address = self.find_gap(size)?.ok_or(ErrorCode::new(
            Level::Fatal,
            Summary::OutOfResource,
            Module::Application,
            ERROR_DESC_OUT_OF_MEMORY,
        ))?;

        let next = self.next_candidate.saturating_add(size);

        if next >= SHAREDMEM_END {
            self.next_candidate = SHAREDMEM_START;
        } else {
            self.next_candidate = next;
        }

        debug!(
            "Mapping memory block at {:p}, size = 0x{:x}, handle = {:?}",
            address as *const u32, size, memory_handle,
        );

        let my_permissions = MemoryPermission::R;
        let other_permissions = MemoryPermission::DontCare;

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

    pub fn unmap(&mut self, block: MappedBlock) -> Result<Handle> {
        unsafe { svc::unmap_memory_block(block.handle.handle(), block.start as usize)? }

        self.next_candidate = block.start;

        Ok(block.handle)
    }

    fn remaining_free(block: &QueryResult, address: usize) -> Option<usize> {
        let offset_in_block = address.checked_sub(block.base_process_virtual_address)?;
        block.size.checked_sub(offset_in_block)
    }

    fn find_gap_within(&mut self, start: usize, end: usize, size: usize) -> Result<Option<usize>> {
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

    fn find_gap(&mut self, size: usize) -> Result<Option<usize>> {
        match self.find_gap_within(self.next_candidate, SHAREDMEM_END, size)? {
            Some(address) => Ok(Some(address)),
            None => self.find_gap_within(SHAREDMEM_START, self.next_candidate, size),
        }
    }
}
