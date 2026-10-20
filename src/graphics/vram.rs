//! Allocator for VRAM memory.

use core::alloc::{AllocError, Allocator, Layout};
use core::ptr::NonNull;

use log::debug;
use talc::{ErrOnOom, Talc, Talck};

/// The recommended alignment for allocations in VRAM.
///
/// Callers are expected to build their [`Layout`] with this alignment.
/// The allocator uses a layout as-is, rather than than widening it.
pub(crate) const ALIGNMENT: usize = 0x80;

pub(crate) struct VramAllocator {
    inner: Talck<spin::Mutex<()>, ErrOnOom>,
}

pub(crate) static ALLOCATOR: VramAllocator = VramAllocator {
    inner: Talc::new(ErrOnOom).lock(),
};

pub(crate) unsafe fn init() -> Result<(), ()> {
    let _span = ALLOCATOR.init()?;
    Ok(())
}

impl VramAllocator {
    pub(crate) unsafe fn init(&self) -> Result<talc::Span, ()> {
        debug!("Initializing VRAM...");

        const VRAM_START: usize = 0x1F00_0000;
        const VRAM_SIZE: usize = 0x60_0000;
        const VRAM_SPAN: talc::Span = talc::Span::from_base_size(VRAM_START as *mut u8, VRAM_SIZE);
        unsafe { self.inner.talc().claim(VRAM_SPAN) }
    }

    pub fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.inner.allocator().allocate(layout)
    }

    /// # Safety
    ///
    /// `ptr` and `layout` must come from a matching [`Self::allocate`] call.
    pub unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { self.inner.allocator().deallocate(ptr, layout) }
    }
}
