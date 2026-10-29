// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::alloc::AllocError;
use core::marker::PhantomData;
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

mod sealed {
    use super::*;

    /// The operations behind [`FramebufferAllocator`].
    pub trait FramebufferAllocator {
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
pub trait FramebufferAllocator: sealed::FramebufferAllocator {}

/// Linear (physically contiguous) main memory.
///
/// Forwards to [`linear`].  The CPU can write framebuffers in linear memory directly.
#[derive(Debug)]
pub enum Linear {}

impl FramebufferAllocator for Linear {}

impl sealed::FramebufferAllocator for Linear {
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
struct Framebuffer<A: FramebufferAllocator> {
    buffer: NonNull<[u8]>,
    /// Layout [`Self::buffer`] was allocated with.
    ///
    /// Kept so that [`Drop`] frees the buffer with exactly the layout it was allocated with.
    /// The allocator may hand back a slice longer than requested, so the length of `buffer` is
    /// not a safe substitute.
    layout: Layout,
    _allocator: PhantomData<A>,
}

impl<A: FramebufferAllocator> Framebuffer<A> {
    /// Layout a framebuffer of `size` bytes is allocated with.
    ///
    /// The alignment the region requires is baked in here rather than inside the allocators, so
    /// that [`Drop`] can free a buffer with exactly the layout it was allocated with.
    fn layout_for_size(size: usize) -> Layout {
        Layout::from_size_align(size, A::ALIGNMENT).unwrap()
    }

    fn new(
        dimensions: ScreenDimensions,
        format: FramebufferColorFormat,
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

    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.buffer.cast().as_ptr()
    }

    /// Make this framebuffer's contents visible to the GPU.
    fn flush(&self, gpu: &mut Gpu) -> Result<()> {
        A::flush(self.buffer, gpu)
    }
}

impl Framebuffer<Linear> {
    fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: `buffer` points at an allocation of `buffer.len()` bytes owned by `self`, and
        // the CPU can write linear memory.
        unsafe { self.buffer.as_mut() }
    }

    /// Fill the framebuffer with a fixed byte value
    fn fill(&mut self, value: u8) {
        self.as_mut_slice().fill(value);
    }

    fn pixels_rgb_mut(&mut self) -> &mut [[u8; 3]] {
        self.as_mut_slice().as_chunks_mut::<3>().0
    }
}

impl<A: FramebufferAllocator> Drop for Framebuffer<A> {
    fn drop(&mut self) {
        // SAFETY: `buffer` was allocated from `A` with `self.layout`.
        unsafe { A::deallocate(self.buffer.cast(), self.layout) }
    }
}

#[derive(Debug)]
struct ScreenConfiguration<A: FramebufferAllocator> {
    format: FramebufferColorFormat,
    dimensions: ScreenDimensions,
    fb0: Framebuffer<A>,
    fb1: Framebuffer<A>,
    active_fb: FramebufferIndex,
}

impl<A: FramebufferAllocator> ScreenConfiguration<A> {
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

        mode |= A::MODE_BITS;
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

    fn buffer_at(&self, index: FramebufferIndex) -> &Framebuffer<A> {
        match index {
            FramebufferIndex::First => &self.fb0,
            FramebufferIndex::Second => &self.fb1,
        }
    }

    fn buffer_at_mut(&mut self, index: FramebufferIndex) -> &mut Framebuffer<A> {
        match index {
            FramebufferIndex::First => &mut self.fb0,
            FramebufferIndex::Second => &mut self.fb1,
        }
    }

    fn active_buffer(&self) -> &Framebuffer<A> {
        self.buffer_at(self.active_fb)
    }

    fn inactive_buffer_mut(&mut self) -> &mut Framebuffer<A> {
        self.buffer_at_mut(self.active_fb.swap())
    }

    fn swap_buffers(&mut self) {
        self.active_fb = self.active_fb.swap();
    }
}

impl ScreenConfiguration<Linear> {
    fn fill_buffer(&mut self, index: FramebufferIndex, value: u8) {
        self.buffer_at_mut(index).fill(value)
    }

    /// Fill the buffer that will be shown after the next [`Self::swap_buffers`].
    fn fill_inactive(&mut self, value: u8) {
        let inactive = self.active_fb.swap();
        self.fill_buffer(inactive, value)
    }
}

/// Double-buffered framebuffers for both screens, allocated from `A`.
///
/// Methods that write pixels with the CPU are only available for [`Linear`] framebuffers.
#[derive(Debug)]
pub struct Graphics<'g, A: FramebufferAllocator = Linear> {
    gpu: &'g mut Gpu,
    stereoscopic: bool,
    top: ScreenConfiguration<A>,
    bottom: ScreenConfiguration<A>,
}

impl<'g> Graphics<'g, Linear> {
    /// Initialize both screens as RGB8, with framebuffers in linear memory.
    ///
    /// Matches libctru's `gfxInitDefault`.
    pub fn init_default(gpu: &'g mut Gpu) -> Result<Self> {
        use FramebufferColorFormat::RGB8;
        Self::init(gpu, RGB8, RGB8)
    }
}

impl<'g, A: FramebufferAllocator> Graphics<'g, A> {
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

        info!(
            "Allocating framebuffers from {} memory",
            core::any::type_name::<A>()
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

    pub fn dimensions(&self, screen: Screen) -> &ScreenDimensions {
        &self.screen_configuration(screen).dimensions
    }

    fn screen_configuration(&self, screen: Screen) -> &ScreenConfiguration<A> {
        match screen {
            Screen::Top => &self.top,
            Screen::Bottom => &self.bottom,
        }
    }

    fn screen_configuration_mut(&mut self, screen: Screen) -> &mut ScreenConfiguration<A> {
        match screen {
            Screen::Top => &mut self.top,
            Screen::Bottom => &mut self.bottom,
        }
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

impl Graphics<'_, Linear> {
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
            .pixels_rgb_mut()
            .fill(pixel);
    }

    pub fn with_buffer_mut<F>(&mut self, screen: Screen, f: F)
    where
        F: FnOnce(&mut [u8]),
    {
        f(self
            .screen_configuration_mut(screen)
            .inactive_buffer_mut()
            .as_mut_slice())
    }

    pub fn with_pixels_rgb_mut<F>(&mut self, screen: Screen, f: F)
    where
        F: FnOnce(&mut [[u8; 3]]),
    {
        f(self
            .screen_configuration_mut(screen)
            .inactive_buffer_mut()
            .pixels_rgb_mut())
    }

    pub fn set_pixel_rgb(&mut self, screen: Screen, x: u16, y: u16, pixel: [u8; 3]) {
        let cfg = self.screen_configuration_mut(screen);
        let offset = x * cfg.dimensions.width + y;

        if let Some(target) = cfg
            .inactive_buffer_mut()
            .pixels_rgb_mut()
            .get_mut(usize::from(offset))
        {
            *target = pixel;
        }
    }
}
