use crate::{os::mem, result::Result, svc};

use core::{
    alloc::{Layout, LayoutErr},
    fmt,
    ptr::NonNull,
};

use linked_list_allocator::LockedHeap;

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

#[global_allocator]
pub(crate) static ALLOCATOR: LockedHeap = LockedHeap::empty();

const HEAP_START: usize = 0x0800_0000;

extern "C" {
    static __heap_size: usize;
}

#[inline]
pub fn heap_size() -> usize {
    unsafe { __heap_size }
}

pub(crate) fn init() -> Result<()> {
    crate::svc::output_debug_string("Initializing heap...\n");

    unsafe {
        let heap_start = svc::control_memory(
            mem::MemoryOperation::allocate(),
            HEAP_START,
            0x0,
            heap_size(),
            mem::MemoryPermission::Rw,
        )?;

        crate::svc::output_debug_string("Mapped heap\n");

        ALLOCATOR.lock().init(heap_start, heap_size());
    }

    Ok(())
}

pub(crate) fn initialized() -> bool {
    ALLOCATOR.lock().bottom() != 0
}

#[derive(Debug)]
pub enum PageAlignErr {
    Alloc,
    Layout(LayoutErr),
}

impl fmt::Display for PageAlignErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Alloc => write!(f, "failed to allocate page aligned memory"),
            Self::Layout(_) => write!(f, "size was not suitable for a page aligned allocation"),
        }
    }
}

#[derive(Debug)]
pub struct PageAlignedBuffer {
    buffer: NonNull<u8>,
    layout: Layout,
}

impl PageAlignedBuffer {
    pub fn allocate(size: usize) -> ::core::result::Result<Self, PageAlignErr> {
        let layout = Self::layout_for_size(size).map_err(PageAlignErr::Layout)?;
        let buffer = ALLOCATOR
            .lock()
            .allocate_first_fit(layout)
            .map_err(|_| PageAlignErr::Alloc)?;
        Ok(PageAlignedBuffer { buffer, layout })
    }

    pub const fn as_ptr(&self) -> *const u8 {
        self.buffer.as_ptr()
    }

    pub fn size(&self) -> usize {
        self.layout.size()
    }

    pub fn layout_for_size(size: usize) -> ::core::result::Result<Layout, LayoutErr> {
        Layout::from_size_align(size, 0x1000)
    }
}

impl Drop for PageAlignedBuffer {
    fn drop(&mut self) {
        unsafe { ALLOCATOR.lock().deallocate(self.buffer, self.layout) }
    }
}
