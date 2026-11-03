// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub mod console;
pub mod framebuffer;
mod screen;

use framebuffer::{Allocator, ColorFormat, Linear};

use crate::result::{ErrorCode, Result};
use crate::services::gsp::gpu::{FramebufferIndex, Gpu, InterruptEvent, Screen, ScreenDimensions};

use log::{debug, info};

/// Double-buffered framebuffers for both screens, allocated from `A`.
///
/// Methods that write pixels with the CPU are only available for [`Linear`] framebuffers.
#[derive(Debug)]
pub struct Graphics<'g, A: Allocator = Linear> {
    gpu: &'g mut Gpu,
    stereoscopic: bool,
    top: screen::SwapChain<A>,
    bottom: screen::SwapChain<A>,
}

impl<'g> Graphics<'g, Linear> {
    /// Initialize both screens as RGB8, with framebuffers in linear memory.
    ///
    /// Matches libctru's `gfxInitDefault`.
    pub fn init_default(gpu: &'g mut Gpu) -> Result<Self> {
        use ColorFormat::RGB8;
        Self::init(gpu, RGB8, RGB8)
    }
}

impl<'g, A: Allocator> Graphics<'g, A> {
    pub fn init(
        gpu: &'g mut Gpu,
        format_top: ColorFormat,
        format_bottom: ColorFormat,
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
            screen::SwapChain::new(Top.dimensions(), format_top).map_err(|_| ERR_SCREEN_ALLOC)?;
        debug!("Configuring bottom screen framebuffer...");
        let bottom = screen::SwapChain::new(Bottom.dimensions(), format_bottom)
            .map_err(|_| ERR_SCREEN_ALLOC)?;

        top.present(Top, gpu)?;
        bottom.present(Bottom, gpu)?;

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
        &self.swap_chain(screen).dimensions
    }

    fn swap_chain(&self, screen: Screen) -> &screen::SwapChain<A> {
        match screen {
            Screen::Top => &self.top,
            Screen::Bottom => &self.bottom,
        }
    }

    fn swap_chain_mut(&mut self, screen: Screen) -> &mut screen::SwapChain<A> {
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

        self.top.present(Screen::Top, self.gpu)?;
        self.bottom.present(Screen::Bottom, self.gpu)?;

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
        self.swap_chain_mut(screen).fill_inactive(value);
    }

    pub fn clear_screen(&mut self, screen: Screen) {
        let chain = self.swap_chain_mut(screen);
        chain.fill_buffer(FramebufferIndex::First, 0);
        chain.fill_buffer(FramebufferIndex::Second, 0);
    }

    #[inline]
    pub fn fill_screen_rgb(&mut self, screen: Screen, pixel: [u8; 3]) {
        self.swap_chain_mut(screen)
            .inactive_buffer_mut()
            .pixels_rgb_mut()
            .fill(pixel);
    }

    pub fn with_buffer_mut<F>(&mut self, screen: Screen, f: F)
    where
        F: FnOnce(&mut [u8]),
    {
        f(self
            .swap_chain_mut(screen)
            .inactive_buffer_mut()
            .as_mut_slice())
    }

    pub fn with_pixels_rgb_mut<F>(&mut self, screen: Screen, f: F)
    where
        F: FnOnce(&mut [[u8; 3]]),
    {
        f(self
            .swap_chain_mut(screen)
            .inactive_buffer_mut()
            .pixels_rgb_mut())
    }

    /// Set the pixel at _(x, y)_ in the off-screen buffer of the selected screen.
    ///
    /// The origin _(0, 0)_ is the bottom-left corner, _x_ grows to the right and _y_ grows upwards.
    /// Pixels outside of the screen are ignored.
    pub fn set_pixel_rgb(&mut self, screen: Screen, x: usize, y: usize, pixel: [u8; 3]) {
        let swap_chain = self.swap_chain_mut(screen);
        // Framebuffers are stored rotated: each line of [`ScreenDimensions::width`] pixels is a
        // column of the screen, running from bottom to top.
        let column_len = usize::from(swap_chain.dimensions.width);
        let columns = usize::from(swap_chain.dimensions.height);

        if x >= columns || y >= column_len {
            return;
        }

        let offset = x * column_len + y;

        if let Some(target) = swap_chain
            .inactive_buffer_mut()
            .pixels_rgb_mut()
            .get_mut(offset)
        {
            *target = pixel;
        }
    }
}
