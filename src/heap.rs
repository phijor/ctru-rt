// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::os::reslimit::process_limits;
use crate::os::WeakHandle;
use crate::result::ERROR_OUT_OF_MEMORY;
use crate::{early_debug, os::mem, result::Result, svc};

use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::{
    alloc::{Layout, LayoutError},
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
const HEAP_SPLIT_CAP: usize = 24 << 20; // 24 MiB
const LINEAR_HEAP_SPLIT_CAP: usize = 32 << 20; // 32 MiB

extern "C" {
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
    __heap_size.store(heap_size, Ordering::Release)
}

unsafe fn set_linear_heap_size(linear_heap_size: usize) {
    __linear_heap_size.store(linear_heap_size, Ordering::Release)
}

#[inline]
fn page_align(size: usize) -> usize {
    size & !0xfff
}

pub(crate) fn init() -> Result<()> {
    early_debug!("Initializing heap...",);

    let memory_remaining = {
        let limits = process_limits(WeakHandle::active_process())?;
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

        unsafe { ALLOCATOR.lock().init(heap_start, heap_size) };

        early_debug!(
            "Initialized heap at {:p}, size = 0x{:08x}",
            heap_start as *const (),
            heap_size,
        );

        unsafe { set_heap_size(heap_size) };

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

        early_debug!(
            "Initialized linear heap at {:p}, size = 0x{:08x}",
            linear_heap_start as *const (),
            linear_heap_size
        );

        unsafe { set_linear_heap_size(linear_heap_size) };
    }

    Ok(())
}

pub(crate) fn initialized() -> bool {
    ALLOCATOR.lock().bottom() != 0
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
pub struct PageAlignedBuffer {
    buffer: Option<NonNull<u8>>,
    layout: Layout,
}

impl PageAlignedBuffer {
    pub const fn null() -> Self {
        Self {
            buffer: None,
            layout: Layout::new::<()>(),
        }
    }

    pub fn allocate(size: usize) -> ::core::result::Result<Self, PageAlignError> {
        let layout = Self::layout_for_size(size).map_err(PageAlignError::Layout)?;
        let buffer = Some(
            ALLOCATOR
                .lock()
                .allocate_first_fit(layout)
                .map_err(|_| PageAlignError::Alloc)?,
        );
        Ok(PageAlignedBuffer { buffer, layout })
    }

    pub const fn as_ptr(&self) -> Option<NonNull<u8>> {
        self.buffer
    }

    pub fn size(&self) -> usize {
        self.layout.size()
    }

    #[allow(deprecated)]
    pub fn layout_for_size(size: usize) -> ::core::result::Result<Layout, LayoutError> {
        Layout::from_size_align(size, 0x1000)
    }
}

impl Default for PageAlignedBuffer {
    fn default() -> Self {
        Self::null()
    }
}

impl Drop for PageAlignedBuffer {
    fn drop(&mut self) {
        if let Some(buffer) = self.buffer {
            unsafe { ALLOCATOR.lock().deallocate(buffer, self.layout) }
        }
    }
}
