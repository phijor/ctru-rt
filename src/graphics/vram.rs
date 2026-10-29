//! Allocator for VRAM memory.

use core::alloc::{AllocError, Layout};
use core::ptr::NonNull;

use talc::source::Claim;
use talc::TalcLock;

use crate::heap;
use crate::sync::arbitrate::RawMutex;

/// The recommended alignment for allocations in VRAM.
///
/// Callers are expected to build their [`Layout`] with this alignment.
/// The allocator uses a layout as-is, rather than than widening it.
pub(crate) const ALIGNMENT: usize = 0x80;

const VRAM_START: usize = 0x1F00_0000;
const VRAM_SIZE: usize = 0x60_0000;

pub(crate) struct VramAllocator {
    inner: TalcLock<RawMutex, Claim>,
}

/// Talc keeps its metadata inside the memory it manages, so claiming VRAM writes to it.  On
/// hardware, processes launched through the Homebrew Launcher cannot write to VRAM, so the
/// [`Claim`] source defers this until the first VRAM allocation instead of running at startup.
pub(crate) static ALLOCATOR: VramAllocator = VramAllocator {
    // SAFETY: VRAM is mapped, and nothing but this allocator hands out VRAM.
    inner: TalcLock::new(unsafe { Claim::new(VRAM_START as *mut u8, VRAM_SIZE) }),
};

impl VramAllocator {
    pub fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        heap::allocate(&self.inner, layout)
    }

    /// # Safety
    ///
    /// `ptr` and `layout` must come from a matching [`Self::allocate`] call.
    pub unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { heap::deallocate(&self.inner, ptr, layout) }
    }
}
