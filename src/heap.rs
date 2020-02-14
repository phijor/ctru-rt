use crate::{
    result::Result,
    svc::{self, mem},
};

use linked_list_allocator::LockedHeap;

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

const HEAP_START: usize = 0x0800_0000;

extern "C" {
    static __heap_size: usize;
}

#[inline]
pub fn heap_size() -> usize {
    unsafe { __heap_size }
}

pub(crate) fn init() -> Result<()> {
    unsafe {
        let heap_start = svc::control_memory(
            HEAP_START,
            0x0,
            heap_size(),
            mem::MemoryOperation::allocate(),
            mem::MemoryPermission::Rw,
        )?;

        ALLOCATOR.lock().init(heap_start, heap_size());
    }

    Ok(())
}
