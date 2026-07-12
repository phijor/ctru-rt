// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::os::reslimit::process_limits;
use crate::os::PseudoHandle;
use crate::result::ERROR_OUT_OF_MEMORY;
use crate::sync::OsMutex;
use crate::{early_debug, os::mem, result::Result, svc};

use core::alloc::Allocator;
use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::{
    alloc::{Layout, LayoutError},
    fmt,
    ptr::NonNull,
};

use talc::{ErrOnOom, Span, Talck};

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

#[global_allocator]
// pub(crate) static ALLOCATOR: Talck<OsMutex, ErrOnOom> = talc::Talc::new(ErrOnOom).lock();
pub(crate) static ALLOCATOR: Talck<spin::Mutex<()>, ErrOnOom> = talc::Talc::new(ErrOnOom).lock();

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
    // TODO: This writes to (R-X) memory lol
    __heap_size.store(heap_size, Ordering::Release)
}

unsafe fn set_linear_heap_size(linear_heap_size: usize) {
    __linear_heap_size.store(linear_heap_size, Ordering::Release)
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

        let heap_span = Span::from_base_size(heap_start as *mut u8, heap_size);

        crate::svc::output_debug_string("Mapped heap");

        let usable_span = unsafe {
            use crate::result::*;
            const ERROR: ErrorCode = ErrorCode::new(
                Level::Fatal,
                Summary::InvalidState,
                Module::Application,
                CommonDescription::InvalidSize as u32,
            );
            ALLOCATOR.talc().claim(heap_span).map_err(|_| ERROR)?
        };

        HEAP_INITIALIZED.store(true, Ordering::Release);

        early_debug!(
            "Initialized heap at {}, size = 0x{:08x}, usable = {}",
            heap_span,
            heap_size,
            usable_span
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

        early_debug!(
            "Initialized linear heap at {:p}, size = 0x{:08x}",
            linear_heap_start as *const (),
            linear_heap_size
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
pub struct PageAlignedBuffer {
    buffer: Option<NonNull<[u8]>>,
}

impl PageAlignedBuffer {
    pub const fn null() -> Self {
        Self { buffer: None }
    }

    pub fn allocate(size: usize) -> ::core::result::Result<Self, PageAlignError> {
        let layout = Self::layout_for_size(size).map_err(PageAlignError::Layout)?;
        let buffer = Some(
            ALLOCATOR
                .allocator()
                .allocate(layout)
                .map_err(|_| PageAlignError::Alloc)?,
        );
        Ok(PageAlignedBuffer { buffer })
    }

    pub const fn as_ptr(&self) -> *const u8 {
        match self.buffer {
            Some(buffer) => buffer.as_ptr() as *const u8,
            None => core::ptr::null(),
        }
    }

    pub fn size(&self) -> usize {
        self.buffer.map_or(0, |buffer| buffer.len())
    }

    pub fn layout_for_size(size: usize) -> ::core::result::Result<Layout, LayoutError> {
        Layout::from_size_align(size, 0x1000)
    }

    unsafe fn layout_for_size_unchecked(size: usize) -> Layout {
        Layout::from_size_align_unchecked(size, 0x1000)
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
            unsafe {
                let layout = Self::layout_for_size_unchecked(buffer.len());
                ALLOCATOR.allocator().deallocate(buffer.cast(), layout)
            };
        }
    }
}
