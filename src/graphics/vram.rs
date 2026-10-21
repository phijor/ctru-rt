//! Allocator for VRAM memory.

use core::alloc::{AllocError, Allocator, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};

use log::debug;
use talc::{ErrOnOom, Talc, Talck};

use crate::sync::arbitrate::RawMutex;

/// The recommended alignment for allocations in VRAM.
///
/// Callers are expected to build their [`Layout`] with this alignment.
/// The allocator uses a layout as-is, rather than than widening it.
pub(crate) const ALIGNMENT: usize = 0x80;

pub(crate) struct VramAllocator {
    inner: Talck<RawMutex, ErrOnOom>,
    /// Whether VRAM has been handed to `inner` yet.
    claimed: AtomicBool,
}

pub(crate) static ALLOCATOR: VramAllocator = VramAllocator {
    inner: Talc::new(ErrOnOom).lock(),
    claimed: AtomicBool::new(false),
};

impl VramAllocator {
    /// Hand VRAM to the allocator, unless that already happened.
    ///
    /// Talc keeps its metadata inside the memory it manages, so claiming VRAM writes to it.  On
    /// hardware, processes launched through the Homebrew Launcher cannot write to VRAM, so this
    /// is deferred until the first VRAM allocation instead of running at startup.
    fn ensure_claimed(&self) -> Result<(), AllocError> {
        const VRAM_START: usize = 0x1F00_0000;
        const VRAM_SIZE: usize = 0x60_0000;
        const VRAM_SPAN: talc::Span = talc::Span::from_base_size(VRAM_START as *mut u8, VRAM_SIZE);

        let mut talc = self.inner.talc();

        // Relaxed: `claimed` is only accessed while holding the lock, which orders the accesses.
        if !self.claimed.load(Ordering::Relaxed) {
            debug!("Claiming VRAM...");
            unsafe { talc.claim(VRAM_SPAN) }.map_err(|()| AllocError)?;
            self.claimed.store(true, Ordering::Relaxed);
        }

        Ok(())
    }

    pub fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.ensure_claimed()?;
        self.inner.allocator().allocate(layout)
    }

    /// # Safety
    ///
    /// `ptr` and `layout` must come from a matching [`Self::allocate`] call.
    pub unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { self.inner.allocator().deallocate(ptr, layout) }
    }
}
