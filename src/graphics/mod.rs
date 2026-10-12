// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::alloc::AllocError;
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
    RGB8,
    RGB565,
    RGB5A1,
    RGBA4,
}

impl FramebufferColorFormat {
    pub(crate) const fn bytes_per_pixel(&self) -> usize {
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
struct FramebufferFormat(u16);

#[derive(Debug)]
struct Framebuffer {
    buffer: Option<NonNull<[u8]>>,
    format: FramebufferColorFormat,
}

impl Framebuffer {
    fn layout_for_size(size: usize) -> Layout {
        Layout::from_size_align(size, 4).unwrap()
    }

    fn new(
        dimensions: ScreenDimensions,
        format: FramebufferColorFormat,
    ) -> core::result::Result<Self, AllocError> {
        let size = usize::from(dimensions.width)
            * usize::from(dimensions.height)
            * format.bytes_per_pixel();
        debug!("Allocating new framebuffer (size = {:#0x})", size);
        let buffer = vram::ALLOCATOR.allocate(Self::layout_for_size(size))?;

        debug!("New framebuffer: {:p}", buffer);

        let mut fb = Self {
            buffer: Some(buffer),
            format,
        };
        fb.fill(0xff);

        Ok(fb)
    }

    pub(crate) fn as_ptr(&self) -> *const u8 {
        match self.buffer {
            Some(buffer) => buffer.cast().as_ptr(),
            None => core::ptr::null(),
        }
    }

    /// Write this framebuffer's dirty cache lines back to memory.
    ///
    /// Required before the GPU scans the buffer out: the writes performed by
    /// [`Self::fill`] and friends only reach the data cache, and a data synchronization
    /// barrier orders them without evicting them.
    fn flush(&self, gpu: &mut Gpu) -> Result<()> {
        match self.buffer {
            Some(buffer) => gpu.flush_data_cache(buffer.cast().as_ptr(), buffer.len()),
            None => Ok(()),
        }
    }

    fn fill(&mut self, value: u8) {
        if let Some(mut buffer) = self.buffer {
            // SAFETY: `buffer` points at an allocation of `buffer.len()` bytes owned by `self`.
            unsafe { buffer.as_mut() }.fill(value);
            crate::sync::mem::data_sync_barrier();
        }
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            unsafe {
                vram::ALLOCATOR.deallocate(buffer.cast(), Self::layout_for_size(buffer.len()))
            }
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
    ) -> core::result::Result<Self, AllocError> {
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

    fn present_buffer(&self, screen: Screen, gpu: &mut Gpu) -> Result<()> {
        let fb = self.active_buffer();

        // Clean our writes out of the data cache before the GPU reads the buffer.
        fb.flush(gpu)?;

        gpu.present_buffer(
            screen,
            self.active_fb,
            fb.as_ptr(),
            fb.as_ptr(), // not a typo, only 2D mode for now
            self.stride(),
            self.mode(screen),
        );

        Ok(())
    }

    fn fill_buffer(&mut self, index: FramebufferIndex, value: u8) {
        self.buffer_at_mut(index).fill(value)
    }

    /// Fill the buffer that will be shown after the next [`Self::swap_buffers`].
    fn fill_inactive(&mut self, value: u8) {
        let inactive = self.active_fb.swap();
        self.fill_buffer(inactive, value)
    }

    fn buffer_at(&self, index: FramebufferIndex) -> &Framebuffer {
        match index {
            FramebufferIndex::First => &self.fb0,
            FramebufferIndex::Second => &self.fb1,
        }
    }

    fn buffer_at_mut(&mut self, index: FramebufferIndex) -> &mut Framebuffer {
        match index {
            FramebufferIndex::First => &mut self.fb0,
            FramebufferIndex::Second => &mut self.fb1,
        }
    }

    fn active_buffer(&self) -> &Framebuffer {
        self.buffer_at(self.active_fb)
    }

    fn inactive_buffer(&self) -> &Framebuffer {
        self.buffer_at(self.active_fb.swap())
    }

    fn swap_buffers(&mut self) {
        self.active_fb = self.active_fb.swap();
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
        use FramebufferColorFormat::RGB8;
        Self::init(gpu, RGB8, RGB8)
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

        top.present_buffer(Top, gpu)?;
        bottom.present_buffer(Bottom, gpu)?;

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
        self.gpu
    }

    fn screen_configuration_mut(&mut self, screen: Screen) -> &mut ScreenConfiguration {
        match screen {
            Screen::Top => &mut self.top,
            Screen::Bottom => &mut self.bottom,
        }
    }

    /// Fill the off-screen buffer of the selected screen.
    ///
    /// Nothing becomes visible until [`Self::swap_buffers`] is called.
    pub fn fill_screen(&mut self, screen: Screen, value: u8) {
        self.screen_configuration_mut(screen).fill_inactive(value);
    }

    /// Make the off-screen buffer of both screens the visible one.
    ///
    /// Each buffer is flushed from the data cache before it is handed to the GPU.
    pub fn swap_buffers(&mut self) -> Result<()> {
        self.top.swap_buffers();
        self.bottom.swap_buffers();

        self.top.present_buffer(Screen::Top, self.gpu)?;
        self.bottom.present_buffer(Screen::Bottom, self.gpu)?;

        Ok(())
    }

    pub fn wait_vblank0(&mut self) -> Result<()> {
        while !self.gpu.next_event()?.contains(InterruptEvent::VBlank0) {}

        Ok(())
    }
}

pub(crate) mod vram {
    use core::alloc::{AllocError, Allocator, Layout};
    use core::ptr::NonNull;

    use log::debug;
    use talc::{ErrOnOom, Talc, Talck};

    /// Alignment guaranteed for every allocation, matching libctru's `vramAlloc`.
    const ALIGNMENT: usize = 0x80;

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
            const VRAM_SPAN: talc::Span =
                talc::Span::from_base_size(VRAM_START as *mut u8, VRAM_SIZE);
            unsafe { self.inner.talc().claim(VRAM_SPAN) }
        }

        pub fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
            let layout = layout.align_to(ALIGNMENT).map_err(|_| AllocError)?;

            self.inner.allocator().allocate(layout)
        }

        /// # Safety
        ///
        /// `ptr` and `layout` must come from a matching [`Self::allocate`] call.
        pub unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
            // Deallocate with the same layout `allocate` widened it to.
            let Ok(layout) = layout.align_to(ALIGNMENT) else {
                return;
            };

            unsafe { self.inner.allocator().deallocate(ptr, layout) }
        }
    }
}
