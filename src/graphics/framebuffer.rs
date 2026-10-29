// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::alloc::AllocError;
use core::marker::PhantomData;
use core::ptr::NonNull;

use crate::heap::linear;
use crate::result::Result;
use crate::services::gsp::gpu::{Gpu, ScreenDimensions};

use alloc::alloc::Layout;
use log::debug;
use num_enum::IntoPrimitive;

#[derive(Debug, IntoPrimitive, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ColorFormat {
    RGBA8,
    RGB8,
    RGB565,
    RGB5A1,
    RGBA4,
}

impl ColorFormat {
    pub(super) const fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::RGBA8 => 4,
            Self::RGB8 => 3,
            Self::RGB565 => 2,
            Self::RGB5A1 => 2,
            Self::RGBA4 => 2,
        }
    }
}

#[derive(Debug)]
struct Format(u16);

mod sealed {
    use super::*;

    /// The operations behind [`Allocator`](super::Allocator).
    pub trait Allocator {
        /// Alignment framebuffers in this region have to be allocated with.
        const ALIGNMENT: usize;

        /// Bits 8..=9 of the framebuffer mode word. These tell GSP where the buffer lives.
        const MODE_BITS: u32;

        fn allocate(layout: Layout) -> core::result::Result<NonNull<[u8]>, AllocError>;

        /// # Safety
        ///
        /// `ptr` and `layout` must come from a matching [`Self::allocate`] call.
        unsafe fn deallocate(ptr: NonNull<u8>, layout: Layout);

        /// Bring a freshly allocated framebuffer into a defined state.
        fn initialize(buffer: NonNull<[u8]>);

        /// Make all writes to `buffer` visible to the GPU.
        fn flush(buffer: NonNull<[u8]>, gpu: &mut Gpu) -> Result<()>;
    }
}

/// A memory region framebuffers can be allocated in.
///
/// This trait is sealed: currently, [`Linear`] is the only implementation.
///
/// In the future, framebuffers in VRAM might be supported.  For this to work the GPS memory fill
/// and transfer commands need to be implemented.  Without those it is not possible to write to VRAM
/// at all (the CPU does not have write access to VRAM).
pub trait Allocator: sealed::Allocator {}

/// Linear (physically contiguous) main memory.
///
/// Forwards to [`linear`].  The CPU can write framebuffers in linear memory directly.
#[derive(Debug)]
pub enum Linear {}

impl Allocator for Linear {}

impl sealed::Allocator for Linear {
    const ALIGNMENT: usize = linear::ALIGNMENT;
    const MODE_BITS: u32 = 0b01 << 8;

    fn allocate(layout: Layout) -> core::result::Result<NonNull<[u8]>, AllocError> {
        linear::ALLOCATOR.allocate(layout)
    }

    unsafe fn deallocate(ptr: NonNull<u8>, layout: Layout) {
        unsafe { linear::ALLOCATOR.deallocate(ptr, layout) }
    }

    fn initialize(mut buffer: NonNull<[u8]>) {
        // SAFETY: `buffer` points at a fresh allocation of `buffer.len()` bytes.
        unsafe { buffer.as_mut() }.fill(0xff);
    }

    /// Issues the data synchronization barrier ([`sync::mem::data_sync_barrier`]) that orders all
    /// preceeding writes to the framebuffer before the cache maintenance operation, then issues a
    /// [`Gpu::flush_data_cache`] IPC command to `GSP:GPU` to write the dirty cache lines back to
    /// memory.
    ///
    /// [`sync::mem::data_sync_barrier`]: crate::sync::mem::data_sync_barrier
    fn flush(buffer: NonNull<[u8]>, gpu: &mut Gpu) -> Result<()> {
        crate::sync::mem::data_sync_barrier();
        gpu.flush_data_cache(buffer.cast().as_ptr(), buffer.len())
    }
}

#[derive(Debug)]
pub(super) struct Framebuffer<A: Allocator> {
    buffer: NonNull<[u8]>,
    /// Layout [`Self::buffer`] was allocated with.
    ///
    /// Kept so that [`Drop`] frees the buffer with exactly the layout it was allocated with.
    /// The allocator may hand back a slice longer than requested, so the length of `buffer` is
    /// not a safe substitute.
    layout: Layout,
    _allocator: PhantomData<A>,
}

impl<A: Allocator> Framebuffer<A> {
    /// Layout a framebuffer of `size` bytes is allocated with.
    ///
    /// The alignment the region requires is baked in here rather than inside the allocators, so
    /// that [`Drop`] can free a buffer with exactly the layout it was allocated with.
    fn layout_for_size(size: usize) -> Layout {
        Layout::from_size_align(size, A::ALIGNMENT).unwrap()
    }

    pub(super) fn new(
        dimensions: ScreenDimensions,
        format: ColorFormat,
    ) -> core::result::Result<Self, AllocError> {
        let size = usize::from(dimensions.width)
            * usize::from(dimensions.height)
            * format.bytes_per_pixel();
        debug!(
            "Allocating new framebuffer (size = {:#0x}, allocator = {})",
            size,
            core::any::type_name::<A>()
        );
        let layout = Self::layout_for_size(size);
        let buffer = A::allocate(layout)?;

        debug!("New framebuffer: {:p}", buffer);

        A::initialize(buffer);

        Ok(Self {
            buffer,
            layout,
            _allocator: PhantomData,
        })
    }

    pub(super) fn as_ptr(&self) -> *const u8 {
        self.buffer.cast().as_ptr()
    }

    /// Make this framebuffer's contents visible to the GPU.
    pub(super) fn flush(&self, gpu: &mut Gpu) -> Result<()> {
        A::flush(self.buffer, gpu)
    }
}

impl Framebuffer<Linear> {
    pub(super) fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: `buffer` points at an allocation of `buffer.len()` bytes owned by `self`, and
        // the CPU can write linear memory.
        unsafe { self.buffer.as_mut() }
    }

    /// Fill the framebuffer with a fixed byte value
    pub(super) fn fill(&mut self, value: u8) {
        self.as_mut_slice().fill(value);
    }

    pub(super) fn pixels_rgb_mut(&mut self) -> &mut [[u8; 3]] {
        self.as_mut_slice().as_chunks_mut::<3>().0
    }
}

impl<A: Allocator> Drop for Framebuffer<A> {
    fn drop(&mut self) {
        // SAFETY: `buffer` was allocated from `A` with `self.layout`.
        unsafe { A::deallocate(self.buffer.cast(), self.layout) }
    }
}
