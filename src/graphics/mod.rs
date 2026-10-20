// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub(crate) mod vram;

use core::alloc::AllocError;
use core::ptr::NonNull;

use crate::heap::linear;
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

/// A memory allocator for framebuffers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramebufferAllocator {
    /// Dedicated video memory.
    ///
    /// Forwards to [`vram`].
    Vram,
    /// Linear (physically contiguous) main memory.
    ///
    /// Forwards to [`linear`].
    Linear,
}

impl FramebufferAllocator {
    fn allocate(&self, layout: Layout) -> core::result::Result<NonNull<[u8]>, AllocError> {
        match self {
            Self::Vram => vram::ALLOCATOR.allocate(layout),
            Self::Linear => linear::ALLOCATOR.allocate(layout),
        }
    }

    /// # Safety
    ///
    /// `ptr` and `layout` must come from a matching [`Self::allocate`] on the same variant.
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        match self {
            Self::Vram => unsafe { vram::ALLOCATOR.deallocate(ptr, layout) },
            Self::Linear => unsafe { linear::ALLOCATOR.deallocate(ptr, layout) },
        }
    }

    /// Alignment framebuffers in this region have to be allocated with.
    const fn alignment(&self) -> usize {
        match self {
            Self::Vram => vram::ALIGNMENT,
            Self::Linear => linear::ALIGNMENT,
        }
    }

    /// Bits 8..=9 of the framebuffer mode word, telling GSP where the buffer lives.
    const fn mode_bits(&self) -> u32 {
        match self {
            Self::Vram => 0b11 << 8,
            Self::Linear => 0b01 << 8,
        }
    }
}

#[derive(Debug)]
struct Framebuffer {
    buffer: Option<NonNull<[u8]>>,
    /// Layout [`Self::buffer`] was allocated with.
    ///
    /// Kept so that [`Drop`] frees the buffer with exactly the layout it was allocated with.
    /// The allocator may hand back a slice longer than requested, so the length of `buffer` is
    /// not a safe substitute.
    layout: Layout,
    format: FramebufferColorFormat,
    allocator: FramebufferAllocator,
}

impl Framebuffer {
    /// Layout a framebuffer of `size` bytes in `region` is allocated with.
    ///
    /// The alignment the region requires is baked in here rather than inside the allocators, so
    /// that [`Drop`] can free a buffer with exactly the layout it was allocated with.
    fn layout_for_size(size: usize, region: FramebufferAllocator) -> Layout {
        Layout::from_size_align(size, region.alignment()).unwrap()
    }

    fn new(
        dimensions: ScreenDimensions,
        format: FramebufferColorFormat,
        allocator: FramebufferAllocator,
    ) -> core::result::Result<Self, AllocError> {
        let size = usize::from(dimensions.width)
            * usize::from(dimensions.height)
            * format.bytes_per_pixel();
        debug!(
            "Allocating new framebuffer (size = {:#0x}, allocator = {:?})",
            size, allocator
        );
        let layout = Self::layout_for_size(size, allocator);
        let buffer = allocator.allocate(layout)?;

        debug!("New framebuffer: {:p}", buffer);

        let mut fb = Self {
            buffer: Some(buffer),
            layout,
            format,
            allocator,
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
    /// Issues the data synchronization barrier ([`sync::mem::data_sync_barrier`]) that orders all
    /// preceeding writes to the framebuffer before the cache maintenance operation.
    ///
    /// If the framebuffer is located in linear memory ([`FramebufferAllocator::Linear`]), it then
    /// issues a [`Gpu::flush_data_cache`] IPC command to `GSP:GPU` to make framebuffer changes
    /// visible to the GPU.
    ///
    /// For framebuffers in VRAM ([`FramebufferAllocator::Vram`]), no IPC command is issued: the
    /// `GSP` service refuses to flush data caches outside of linear memory.  The data
    /// synchronization barrier is still needed there, since VRAM is write-combined and pixel writes
    /// may still sit in the write buffer.
    fn flush(&self, gpu: &mut Gpu) -> Result<()> {
        crate::sync::mem::data_sync_barrier();

        if let FramebufferAllocator::Vram = self.allocator {
            return Ok(());
        }

        match self.buffer {
            Some(buffer) => gpu.flush_data_cache(buffer.cast().as_ptr(), buffer.len()),
            None => Ok(()),
        }
    }

    /// Run `f` on this framebuffer's memory, if it is allocated.
    #[inline]
    fn with_mut<F>(&mut self, f: F)
    where
        F: FnOnce(&mut NonNull<[u8]>),
    {
        if let Some(mut buffer) = self.buffer {
            f(&mut buffer);
        }
    }

    /// Fill the framebuffer with a fixed byte value
    fn fill(&mut self, value: u8) {
        self.with_mut(|buffer| {
            // SAFETY: `buffer` points at an allocation of `buffer.len()` bytes owned by `self`.
            unsafe { buffer.as_mut() }.fill(value);
        });
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            // SAFETY: `buffer` was allocated from `self.allocator` with `self.layout`.
            unsafe { self.allocator.deallocate(buffer.cast(), self.layout) }
        }
    }
}

#[derive(Debug)]
struct ScreenConfiguration {
    format: FramebufferColorFormat,
    dimensions: ScreenDimensions,
    allocator: FramebufferAllocator,
    fb0: Framebuffer,
    fb1: Framebuffer,
    active_fb: FramebufferIndex,
}

impl ScreenConfiguration {
    fn new(
        dimensions: ScreenDimensions,
        format: FramebufferColorFormat,
        allocator: FramebufferAllocator,
    ) -> core::result::Result<Self, AllocError> {
        let fb0 = Framebuffer::new(dimensions, format, allocator)?;
        let fb1 = Framebuffer::new(dimensions, format, allocator)?;

        Ok(Self {
            format,
            dimensions,
            allocator,
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

        mode |= self.allocator.mode_bits();
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

    fn inactive_buffer_mut(&mut self) -> &mut Framebuffer {
        self.buffer_at_mut(self.active_fb.swap())
    }

    fn swap_buffers(&mut self) {
        self.active_fb = self.active_fb.swap();
    }
}

#[derive(Debug)]
pub struct Graphics<'g> {
    gpu: &'g mut Gpu,
    stereoscopic: bool,
    top: ScreenConfiguration,
    bottom: ScreenConfiguration,
}

impl<'g> Graphics<'g> {
    /// Initialize both screens as RGB8, with framebuffers in linear memory.
    ///
    /// Matches libctru's `gfxInitDefault`.
    pub fn init_default(gpu: &'g mut Gpu) -> Result<Self> {
        use FramebufferColorFormat::RGB8;
        Self::init(gpu, RGB8, RGB8, FramebufferAllocator::Linear)
    }

    pub fn init(
        gpu: &'g mut Gpu,
        format_top: FramebufferColorFormat,
        format_bottom: FramebufferColorFormat,
        allocator: FramebufferAllocator,
    ) -> Result<Self> {
        use crate::result::{CommonDescription, Level, Module, Summary};
        use Screen::{Bottom, Top};
        const ERR_SCREEN_ALLOC: ErrorCode = ErrorCode::new(
            Level::Usage,
            Summary::OutOfResource,
            Module::Application,
            CommonDescription::InvalidResultValue as u32,
        );

        info!("Allocating framebuffers from {:?} memory", allocator);

        debug!("Configuring top screen framebuffer...");
        let top = ScreenConfiguration::new(Top.dimensions(), format_top, allocator)
            .map_err(|_| ERR_SCREEN_ALLOC)?;
        debug!("Configuring bottom screen framebuffer...");
        let bottom = ScreenConfiguration::new(Bottom.dimensions(), format_bottom, allocator)
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

    #[inline]
    pub fn fill_screen_rgb(&mut self, screen: Screen, pixel: [u8; 3]) {
        self.screen_configuration_mut(screen)
            .inactive_buffer_mut()
            .with_mut(|buffer| unsafe {
                let (chunks, _) = buffer.as_mut().as_chunks_mut::<3>();
                chunks.fill(pixel);
            });
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
