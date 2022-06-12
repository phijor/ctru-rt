// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::ptr::NonNull;

use crate::heap::LINEAR_ALLOCATOR;
use crate::result::{ErrorCode, Result};
use crate::services::gsp::gpu::{FramebufferIndex, Gpu, InterruptEvent, Screen, ScreenDimensions};

use ctru_rt_macros::EnumCast;

use alloc::alloc::Layout;
use log::{debug, info};

#[derive(Debug, EnumCast, Clone, Copy, PartialEq, Eq)]
#[enum_cast(value_type = "u8")]
pub enum FramebufferColorFormat {
    RGBA8,
    BGR8,
    RGB565,
    RGB5A1,
    RGBA4,
}

impl FramebufferColorFormat {
    pub(crate) const fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::RGBA8 => 4,
            Self::BGR8 => 3,
            _ => 2,
        }
    }
}

#[derive(Debug)]
struct FramebufferFormat(u16);

#[derive(Debug)]
struct Framebuffer {
    buffer: Option<NonNull<u8>>,
    size: usize,
    format: FramebufferColorFormat,
}

impl Framebuffer {
    fn layout_for_size(size: usize) -> Layout {
        Layout::from_size_align(size, 4).unwrap()
    }

    fn new(
        dimensions: ScreenDimensions,
        format: FramebufferColorFormat,
    ) -> core::result::Result<Self, ()> {
        let size = usize::from(dimensions.width)
            * usize::from(dimensions.height)
            * format.bytes_per_pixel();
        debug!("Allocating new framebuffer (size = {:#0x})", size);
        let buffer = LINEAR_ALLOCATOR
            .lock()
            .allocate_first_fit(Self::layout_for_size(size))?;

        debug!("New framebuffer: {:p}", buffer);

        let buffer = Some(buffer);

        Ok(Self {
            buffer,
            size,
            format,
        })
    }

    pub(crate) fn as_ptr(&self) -> *const u8 {
        match self.buffer {
            Some(buffer) => buffer.as_ptr(),
            None => core::ptr::null(),
        }
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            unsafe {
                LINEAR_ALLOCATOR
                    .lock()
                    .deallocate(buffer, Self::layout_for_size(self.size))
            }
        }
    }
}

#[derive(Debug)]
struct ScreenConfiguration {
    color_format: FramebufferColorFormat,
    dimensions: ScreenDimensions,
    fb0: Framebuffer,
    fb1: Framebuffer,
    active_fb: FramebufferIndex,
}

impl ScreenConfiguration {
    fn new(
        dimensions: ScreenDimensions,
        color_format: FramebufferColorFormat,
    ) -> core::result::Result<Self, ()> {
        let fb0 = Framebuffer::new(dimensions, color_format)?;
        let fb1 = Framebuffer::new(dimensions, color_format)?;

        Ok(Self {
            color_format,
            dimensions,
            fb0,
            fb1,
            active_fb: FramebufferIndex::First,
        })
    }

    fn stride(&self) -> u32 {
        (self.dimensions.width as u32) * (self.color_format.bytes_per_pixel() as u32)
    }

    const fn format(&self, screen: Screen) -> u32 {
        let mut format: u32;

        format = self.color_format.to_value() as u32;

        if let Screen::Top = screen {
            format |= 1 << 4; // Scan-line doubling
            format |= 1 << 6; // 2D mode
        }

        format |= 0b11_0000_0000; // linear mem buffers
        format
    }

    fn present_buffer(&self, screen: Screen, gpu: &mut Gpu) {
        gpu.present_buffer(
            screen,
            self.active_fb,
            self.fb0.as_ptr(),
            self.fb0.as_ptr(), // not a typo, only 2D mode for now
            self.stride(),
            self.format(screen),
        )
    }
}

#[derive(Debug)]
pub struct Grapics<'g> {
    gpu: &'g mut Gpu,
    stereoscopic: bool,
    top: ScreenConfiguration,
    bottom: ScreenConfiguration,
}

impl<'g> Grapics<'g> {
    pub fn init_default(gpu: &'g mut Gpu) -> Result<Self> {
        use FramebufferColorFormat::BGR8;
        Self::init(gpu, BGR8, BGR8)
    }

    pub fn init(
        gpu: &'g mut Gpu,
        format_top: FramebufferColorFormat,
        format_bottom: FramebufferColorFormat,
    ) -> Result<Self> {
        use crate::result::{CommonDescription, Level, Module, Summary};
        use Screen::{Bottom, Top};
        const ERR_SCREEN_ALLOC: ErrorCode = ErrorCode::new(
            Level::Usage,
            Summary::OutOfResource,
            Module::Application,
            CommonDescription::InvalidResultValue.to_value(),
        );

        debug!("Configuring top screen framebuffer...");
        let top =
            ScreenConfiguration::new(Top.dimensions(), format_top).map_err(|_| ERR_SCREEN_ALLOC)?;
        debug!("Configuring bottom screen framebuffer...");
        let bottom = ScreenConfiguration::new(Bottom.dimensions(), format_bottom)
            .map_err(|_| ERR_SCREEN_ALLOC)?;

        top.present_buffer(Top, gpu);
        bottom.present_buffer(Bottom, gpu);

        gpu.wait_for_event(InterruptEvent::VBlank0)?;

        info!("Turning on LCD...");
        gpu.set_lcd_force_blank(0x00)?;

        Ok(Self {
            gpu,
            stereoscopic: false,
            top,
            bottom,
        })
    }

    pub fn gpu(&'g mut self) -> &'g mut Gpu {
        &mut self.gpu
    }

    pub fn wait_vblank0(&mut self) -> Result<()> {
        while !self.gpu.next_event()?.contains(InterruptEvent::VBlank0) {}

        Ok(())
    }
}

pub(crate) mod vram {
    use core::ptr::NonNull;

    use alloc::alloc::Layout;

    use linked_list_allocator::LockedHeap;

    pub(crate) struct VramAllocator {
        inner: LockedHeap,
    }

    pub(crate) static ALLOCATOR: VramAllocator = VramAllocator {
        inner: LockedHeap::empty(),
    };

    pub(crate) fn init() {
        ALLOCATOR.init()
    }

    impl VramAllocator {
        pub(crate) fn init(&self) {
            const VRAM_START: usize = 0x1F00_0000;
            const VRAM_SIZE: usize = 0x60_0000;
            unsafe { self.inner.lock().init(VRAM_START, VRAM_SIZE) }
        }

        pub fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, ()> {
            let layout = layout.align_to(16).map_err(drop)?;

            self.inner.lock().allocate_first_fit(layout)
        }

        pub unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
            self.inner.lock().deallocate(ptr, layout)
        }
    }
}
