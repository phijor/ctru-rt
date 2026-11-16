// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::os::PseudoHandle;
use crate::os::reslimit::process_limits;
use crate::result::ERROR_OUT_OF_MEMORY;
use crate::sync::arbitrate::RawMutex;
use crate::{early_debug, os::mem, result::Result, svc};

use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::{
    alloc::{AllocError, Layout, LayoutError},
    fmt,
    ptr::NonNull,
};

use talc::TalcLock;
use talc::source::{Manual, Source};

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

#[global_allocator]
pub(crate) static ALLOCATOR: TalcLock<RawMutex, Manual> = TalcLock::new(Manual);

/// Allocate `layout` from `talc`.
///
/// Talc only implements `allocator_api2`'s `Allocator` trait, not the one in `core`, so this
/// mirrors its implementation using `core` types instead.
pub(crate) fn allocate<S: Source>(
    talc: &TalcLock<RawMutex, S>,
    layout: Layout,
) -> core::result::Result<NonNull<[u8]>, AllocError> {
    if layout.size() == 0 {
        return Ok(NonNull::slice_from_raw_parts(NonNull::dangling(), 0));
    }

    // SAFETY: `layout` has a nonzero size.
    unsafe { talc.lock().allocate(layout) }
        .map(|ptr| NonNull::slice_from_raw_parts(ptr, layout.size()))
        .ok_or(AllocError)
}

/// # Safety
///
/// `ptr` and `layout` must come from a matching [`allocate`] call on the same `talc`.
pub(crate) unsafe fn deallocate<S: Source>(
    talc: &TalcLock<RawMutex, S>,
    ptr: NonNull<u8>,
    layout: Layout,
) {
    if layout.size() != 0 {
        unsafe { talc.lock().deallocate(ptr.as_ptr(), layout) }
    }
}

/// Allocator for *linear* memory.
///
/// Linear memory is physically contiguous and sits at a fixed virtual-to-physical offset.
/// Other bus masters (in particular the GPU) can address allocations inside of linear memory
/// without having to do any address translation.
///
/// Use GSP commands [`flush_data_cache`][flush_data_cache] and
/// [`invalidate_data_cache`][invalidate_data_cache] to make modifications by the CPU and GPU
/// (respectively) visible.
///
/// [invalidate_data_cache]: crate::services::gsp::gpu::Gpu::invalidate_data_cache
/// [flush_data_cache]: crate::services::gsp::gpu::Gpu::flush_data_cache
pub mod linear {
    use core::alloc::{AllocError, Layout};
    use core::ptr::NonNull;

    use talc::TalcLock;
    use talc::source::Manual;

    use crate::sync::arbitrate::RawMutex;

    /// Recommended alignment for allocations in linear memory.
    ///
    /// Callers are expected to build their [`Layout`] with this alignment.
    /// The allocator uses a layout as-is, rather than than widening it.
    pub const ALIGNMENT: usize = 0x80;

    pub struct LinearAllocator {
        inner: TalcLock<RawMutex, Manual>,
    }

    pub static ALLOCATOR: LinearAllocator = LinearAllocator {
        inner: TalcLock::new(Manual),
    };

    impl LinearAllocator {
        /// Returns the end of the claimed heap, or [`None`] if `size` is too small to claim.
        ///
        /// # Safety
        ///
        /// `base..base + size` must be memory that is mapped, writable, and owned by this allocator.
        pub(super) unsafe fn claim(&self, base: *mut u8, size: usize) -> Option<NonNull<u8>> {
            unsafe { self.inner.lock().claim(base, size) }
        }

        pub fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
            super::allocate(&self.inner, layout)
        }

        /// # Safety
        ///
        /// `ptr` and `layout` must come from a matching [`Self::allocate`] call.
        pub unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
            unsafe { super::deallocate(&self.inner, ptr, layout) }
        }
    }
}

const HEAP_START: usize = 0x0800_0000;
const HEAP_SPLIT_CAP: usize = 24 << 20; // 24 MiB
const LINEAR_HEAP_SPLIT_CAP: usize = 32 << 20; // 32 MiB

unsafe extern "C" {
    static __heap_size: AtomicUsize;
    static __linear_heap_size: AtomicUsize;
}

#[inline]
pub fn heap_size() -> usize {
    unsafe { &__heap_size }.load(Ordering::Acquire)
}

#[inline]
pub(crate) fn linear_heap_size() -> usize {
    unsafe { &__linear_heap_size }.load(Ordering::Acquire)
}

unsafe fn set_heap_size(heap_size: usize) {
    unsafe {
        // TODO: This writes to (R-X) memory lol
        __heap_size.store(heap_size, Ordering::Release)
    }
}

unsafe fn set_linear_heap_size(linear_heap_size: usize) {
    unsafe { __linear_heap_size.store(linear_heap_size, Ordering::Release) }
}

#[inline]
fn page_align(size: usize) -> usize {
    size & !0xfff
}

static HEAP_INITIALIZED: AtomicBool = AtomicBool::new(false);

pub(crate) fn init() -> Result<()> {
    early_debug!("Initializing heap...",);

    let memory_remaining = {
        let limits = process_limits(PseudoHandle::ActiveProcess)?;
        limits.memory_allocatable().remaining()?
    };

    let memory_remaining = if memory_remaining < 0 {
        return Err(ERROR_OUT_OF_MEMORY);
    } else {
        page_align(memory_remaining as usize)
    };

    let heap_size = heap_size();
    let linear_heap_size = linear_heap_size();

    let total_memory = heap_size + linear_heap_size;

    if total_memory > memory_remaining {
        return Err(ERROR_OUT_OF_MEMORY);
    }

    let (heap_size, linear_heap_size) = match (
        NonZeroUsize::new(heap_size),
        NonZeroUsize::new(linear_heap_size),
    ) {
        (None, None) => {
            let half = page_align(memory_remaining / 2);
            let heap_size = (memory_remaining - half).min(HEAP_SPLIT_CAP);
            let linear_heap_size = memory_remaining - heap_size;

            if linear_heap_size > LINEAR_HEAP_SPLIT_CAP {
                (
                    memory_remaining - LINEAR_HEAP_SPLIT_CAP,
                    LINEAR_HEAP_SPLIT_CAP,
                )
            } else {
                (heap_size, linear_heap_size)
            }
        }
        (Some(_heap_size), None) => (heap_size, memory_remaining - heap_size),
        (None, Some(_linear_heap_size)) => (memory_remaining - linear_heap_size, linear_heap_size),
        _ => (heap_size, linear_heap_size),
    };

    {
        let heap_start = unsafe {
            svc::control_memory(
                mem::MemoryOperation::allocate(),
                HEAP_START,
                0x0,
                heap_size,
                mem::MemoryPermission::Rw,
            )?
        };

        crate::svc::output_debug_string("Mapped heap");

        let heap_end = unsafe {
            use crate::result::*;
            const ERROR: ErrorCode = ErrorCode::new(
                Level::Fatal,
                Summary::InvalidState,
                Module::Application,
                CommonDescription::InvalidSize as u32,
            );
            ALLOCATOR
                .lock()
                .claim(heap_start as *mut u8, heap_size)
                .ok_or(ERROR)?
        };

        HEAP_INITIALIZED.store(true, Ordering::Release);

        early_debug!(
            "Initialized heap at {:p}, size = 0x{:08x}, end = {:p}",
            heap_start as *const (),
            heap_size,
            heap_end
        );

        heap_start
    };

    {
        const ADDR_DONT_CARE: usize = 0x0;
        let linear_heap_start = unsafe {
            svc::control_memory(
                mem::MemoryOperation::allocate().linear(),
                ADDR_DONT_CARE,
                ADDR_DONT_CARE,
                linear_heap_size,
                mem::MemoryPermission::Rw,
            )?
        };

        let linear_heap_end = unsafe {
            use crate::result::*;
            const ERROR: ErrorCode = ErrorCode::new(
                Level::Fatal,
                Summary::InvalidState,
                Module::Application,
                CommonDescription::InvalidSize as u32,
            );
            linear::ALLOCATOR
                .claim(linear_heap_start as *mut u8, linear_heap_size)
                .ok_or(ERROR)?
        };

        early_debug!(
            "Initialized linear heap at {:p}, size = 0x{:08x}, end = {:p}",
            linear_heap_start as *const (),
            linear_heap_size,
            linear_heap_end
        );
    }

    Ok(())
}

pub(crate) fn initialized() -> bool {
    HEAP_INITIALIZED.load(Ordering::Acquire)
}

#[derive(Debug)]
pub enum PageAlignError {
    Alloc,
    Layout(#[allow(deprecated)] LayoutError),
}

impl fmt::Display for PageAlignError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Alloc => write!(f, "failed to allocate page aligned memory"),
            Self::Layout(_) => write!(f, "size was not suitable for a page aligned allocation"),
        }
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct PageAlignedBuffer {
    buffer: NonNull<[u8]>,
}

impl PageAlignedBuffer {
    pub fn allocate(size: usize) -> ::core::result::Result<Self, PageAlignError> {
        let layout = Self::layout_for_size(size).map_err(PageAlignError::Layout)?;
        let buffer = allocate(&ALLOCATOR, layout).map_err(|_| PageAlignError::Alloc)?;
        Ok(PageAlignedBuffer { buffer })
    }

    pub const fn as_mut_ptr(&mut self) -> *mut [u8] {
        self.buffer.as_ptr()
    }

    pub const fn size(&self) -> usize {
        self.buffer.len()
    }

    pub fn layout_for_size(size: usize) -> ::core::result::Result<Layout, LayoutError> {
        Layout::from_size_align(size, 0x1000)
    }

    unsafe fn layout_for_size_unchecked(size: usize) -> Layout {
        unsafe { Layout::from_size_align_unchecked(size, 0x1000) }
    }
}

impl Drop for PageAlignedBuffer {
    fn drop(&mut self) {
        let Self { buffer } = self;
        unsafe {
            let layout = Self::layout_for_size_unchecked(buffer.len());
            deallocate(&ALLOCATOR, buffer.cast(), layout)
        };
    }
}
