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
    // chunks: VecDeque<Chunk>,
    // end: PageAdress,
}
const SHAREDMEM_START: usize = 0x10_000_000;
const SHAREDMEM_END: usize = 0x14_000_000;

impl SharedMemoryMapper {
    pub fn new() -> Self {
        Self {
            next_candidate: SHAREDMEM_START,
            // chunks: core::iter::once(Chunk {
            //     start: PageAdress(SHAREDMEM_START),
            //     mapped: None,
            // })
            // .collect(),
            // end: PageAdress(SHAREDMEM_END),
        }
    }

    // pub fn map_memory(&mut self, memory_handle: Handle, size: usize) -> Result<&'_ MappedChunk> {
    pub fn map(&mut self, memory_handle: Handle, size: usize) -> Result<MappedBlock> {
        let size = (size + 0xFFF) & !0xFFF;

        let address = self.find_gap(size)?.ok_or(ErrorCode::new(
            Level::Fatal,
            Summary::OutOfResource,
            Module::Application,
            1011,
        ))?;

        let next = self.next_candidate.saturating_add(size);

        if next >= SHAREDMEM_END {
            self.next_candidate = SHAREDMEM_START;
        } else {
            self.next_candidate = next;
        }

        let my_permissions = MemoryPermission::R;
        let other_permissions = MemoryPermission::DontCare;

        debug!(
            "Mapping memory block at {:p}, size = 0x{:x}, handle = {:?}, perm = ({:x}, {:x})",
            address as *const u32,
            size,
            memory_handle,
            my_permissions as u32,
            other_permissions as u32,
        );

        // TODO: figure out how/when this is sound.
        // Probably never at the moment; we need to tie the lifetime of the slice to the lifetime
        // of `memory_handle`.
        let _ = unsafe {
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

        // let mut chunks = self.chunks.iter_mut().enumerate().peekable();
        // loop {
        //     match chunks.next() {
        //         Some((index, chunk)) => {
        //             if chunk.mapped.is_some() {
        //                 continue;
        //             }

        //             let next_start = chunks
        //                 .peek()
        //                 .map(|(_, next)| next.start)
        //                 .unwrap_or(self.end);
        //             let available = next_start.offset_from(chunk.start);

        //             match available.checked_sub(size) {
        //                 None => continue,
        //                 Some(0) => {
        //                     let _ = chunk.mapped.replace(unimplemented!());
        //                 }
        //                 Some(rest) => unimplemented!(),
        //             }
        //         }
        //         None => return None,
        //     }
        // }
    }
}

// pub(crate) static ALLOCATOR: SharedMemoryMapper = SharedMemoryMapper::new();

// pub(crate) fn init() {
//     unsafe {
//         unimplemented!()
//         // ALLOCATOR.lock().init(SHAREDMEM_START, SHAREDMEM_SIZE);
//     }
// }

// #[derive(Debug)]
// pub struct SharedMemoryBuffer<T> {
//     data: NonNull<T>,
// }
//
// impl<T: Send> SharedMemoryBuffer<T> {
//     pub fn new(value: T) -> Self {
//         let data = unsafe {
//             let data = promote_non_null::<T>(
//                 ALLOCATOR
//                     .lock()
//                     .allocate_first_fit(aligned_layout_for_value(&value))
//                     .expect("Failed to allocate shared memory buffer"),
//             );
//
//             ptr::write(data.as_ptr(), value);
//             data
//         };
//
//         Self { data }
//     }
// }
//
// impl<T> Drop for SharedMemoryBuffer<T> {
//     fn drop(&mut self) {
//         unsafe {
//             ptr::drop_in_place(self.data.as_ptr());
//             ALLOCATOR.lock().deallocate(
//                 demote_non_null(self.data),
//                 aligned_layout_for_value(self.deref()),
//             )
//         }
//     }
// }
//
// impl<T> Deref for SharedMemoryBuffer<T> {
//     type Target = T;
//
//     fn deref(&self) -> &Self::Target {
//         unsafe { self.data.as_ref() }
//     }
// }
//
// impl<T> DerefMut for SharedMemoryBuffer<T> {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         unsafe { self.data.as_mut() }
//     }
// }
//
// unsafe fn promote_non_null<T>(ptr: NonNull<u8>) -> NonNull<T> {
//     NonNull::new_unchecked(ptr.as_ptr() as *mut T)
// }
//
// unsafe fn demote_non_null<T>(ptr: NonNull<T>) -> NonNull<u8> {
//     NonNull::new_unchecked(ptr.as_ptr() as *mut u8)
// }
//
// unsafe fn aligned_layout_for_value<T>(value: &T) -> Layout {
//     Layout::from_size_align_unchecked(
//         core::mem::size_of_val(value),
//         core::mem::align_of_val(value).max(0x1000),
//     )
// }
