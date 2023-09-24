// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::ptr::NonNull;

use crate::result::{ErrorCode, Result};
use crate::services::gsp::gpu::{FramebufferIndex, Gpu, InterruptEvent, Screen, ScreenDimensions};

use alloc::alloc::Layout;
use log::{debug, info};
use num_enum::IntoPrimitive;

#[derive(Debug, IntoPrimitive, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
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
        let buffer = vram::ALLOCATOR.allocate(Self::layout_for_size(size))?;

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
            unsafe { vram::ALLOCATOR.deallocate(buffer, Self::layout_for_size(self.size)) }
        }
    }
}

#[derive(Debug)]
struct ScreenConfiguration {
    format: FramebufferColorFormat,
    dimensions: ScreenDimensions,
    fb0: Framebuffer,
    fb1: Framebuffer,
    active_fb: FramebufferIndex,
}

impl ScreenConfiguration {
    fn new(
        dimensions: ScreenDimensions,
        format: FramebufferColorFormat,
    ) -> core::result::Result<Self, ()> {
        let fb0 = Framebuffer::new(dimensions, format)?;
        let fb1 = Framebuffer::new(dimensions, format)?;

        Ok(Self {
            format,
            dimensions,
            fb0,
            fb1,
            active_fb: FramebufferIndex::First,
        })
    }

    fn stride(&self) -> u32 {
        (self.dimensions.width as u32) * (self.format.bytes_per_pixel() as u32)
    }

    const fn mode(&self, screen: Screen) -> u32 {
        let mut mode = (self.format as u8) as u32;
        if let Screen::Top = screen {
            mode |= 1 << 6; // 2D mode
        }

        mode |= 0b11_0000_0000; // linear mem buffers
        mode
    }

    fn present_buffer(&self, screen: Screen, gpu: &mut Gpu) {
        gpu.present_buffer(
            screen,
            self.active_fb,
            self.fb0.as_ptr(),
            self.fb0.as_ptr(), // not a typo, only 2D mode for now
            self.stride(),
            self.mode(screen),
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
            CommonDescription::InvalidResultValue as u32,
        );

        debug!("Configuring top screen framebuffer...");
        let top =
            ScreenConfiguration::new(Top.dimensions(), format_top).map_err(|_| ERR_SCREEN_ALLOC)?;
        debug!("Configuring bottom screen framebuffer...");
        let bottom = ScreenConfiguration::new(Bottom.dimensions(), format_bottom)
            .map_err(|_| ERR_SCREEN_ALLOC)?;

        top.present_buffer(Top, gpu);
        bottom.present_buffer(Bottom, gpu);

        while !gpu.next_event()?.contains(InterruptEvent::VBlank0) {}

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
