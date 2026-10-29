// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::alloc::AllocError;

use crate::result::Result;
use crate::services::gsp::gpu::{FramebufferIndex, Gpu, Screen, ScreenDimensions};

use super::framebuffer::{Allocator, ColorFormat, Framebuffer, Linear};

#[derive(Debug)]
pub(super) struct SwapChain<A: Allocator> {
    format: ColorFormat,
    pub(super) dimensions: ScreenDimensions,
    /// Framebuffers, indexed by [`FramebufferIndex`].
    fb: [Framebuffer<A>; 2],
    active_fb: FramebufferIndex,
}

impl<A: Allocator> SwapChain<A> {
    pub(super) fn new(
        dimensions: ScreenDimensions,
        format: ColorFormat,
    ) -> core::result::Result<Self, AllocError> {
        let fb = [
            Framebuffer::new(dimensions, format)?,
            Framebuffer::new(dimensions, format)?,
        ];

        Ok(Self {
            format,
            dimensions,
            fb,
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

    pub(super) fn present(&self, screen: Screen, gpu: &mut Gpu) -> Result<()> {
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
        &self.fb[usize::from(u8::from(index))]
    }

    fn buffer_at_mut(&mut self, index: FramebufferIndex) -> &mut Framebuffer<A> {
        &mut self.fb[usize::from(u8::from(index))]
    }

    fn active_buffer(&self) -> &Framebuffer<A> {
        self.buffer_at(self.active_fb)
    }

    pub(super) fn inactive_buffer_mut(&mut self) -> &mut Framebuffer<A> {
        self.buffer_at_mut(self.active_fb.swap())
    }

    pub(super) fn swap_buffers(&mut self) {
        self.active_fb = self.active_fb.swap();
    }
}

impl SwapChain<Linear> {
    fn fill_buffer(&mut self, index: FramebufferIndex, value: u8) {
        self.buffer_at_mut(index).fill(value)
    }

    /// Fill the buffer that will be shown after the next [`Self::swap_buffers`].
    pub(super) fn fill_inactive(&mut self, value: u8) {
        let inactive = self.active_fb.swap();
        self.fill_buffer(inactive, value)
    }
}
