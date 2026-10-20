// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo};

use fixedbitset::FixedBitSet;

use ctru_rt::{
    entry,
    graphics::{FramebufferAllocator, Graphics},
    ports::srv::Srv,
    result::Result,
    services::{
        gsp::gpu::{Gpu, Screen, ScreenDimensions},
        hid::Hid,
    },
    svc,
};
use log::{error, info};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    use ctru_rt::{debug::SvcDebugLog, svc::UserBreakReason};
    let _ = writeln!(SvcDebugLog, "[PANIC] {}", info);

    svc::user_break(UserBreakReason::Panic)
}

const FRAMEBUFFER_ALLOCATOR: FramebufferAllocator = FramebufferAllocator::Linear;

struct GameOfLife {
    next: FixedBitSet,
    prev: FixedBitSet,
    /// Number of cells per framebuffer column.
    stride: usize,
    /// Number of framebuffer columns.
    columns: usize,
}

impl GameOfLife {
    fn new(dimensions: &ScreenDimensions) -> Self {
        // Framebuffers are stored column-major: `width` cells make up one column,
        // of which there are `height`.
        let stride = usize::from(dimensions.width);
        let columns = usize::from(dimensions.height);
        let size = stride * columns;

        Self {
            next: FixedBitSet::with_capacity(size),
            prev: FixedBitSet::with_capacity(size),
            stride,
            columns,
        }
    }

    fn seed(&mut self, seed: u32) {
        // let mut rng = 0x1234_5678u32;
        let mut rng = seed;
        for idx in 0..self.prev.len() {
            rng = rng.wrapping_mul(0x41C6_4E6D).wrapping_add(0x0000_6073);
            // The low bits of an LCG hardly vary at all, take one from the middle.
            self.prev.set(idx, (rng >> 16) & 0b1 == 0);
        }
    }

    /// Number of live cells in row `row` of the three adjacent columns starting at
    /// `left`, `here` and `right`.
    #[inline]
    fn column_sum(
        cells: &FixedBitSet,
        left: usize,
        here: usize,
        right: usize,
        row: usize,
    ) -> usize {
        usize::from(cells.contains(left + row))
            + usize::from(cells.contains(here + row))
            + usize::from(cells.contains(right + row))
    }

    fn update(&mut self) {
        let Self {
            prev,
            next,
            stride,
            columns,
        } = self;
        let (stride, columns) = (*stride, *columns);
        let size = stride * columns;

        // Walk the indices of the first cell of the column left of, at, and right of
        // the one being updated.
        let mut left = size - stride;
        let mut here = 0;
        let mut right = stride;

        for _ in 0..columns {
            // The 3x3 neighbourhood of a cell is three of these column sums, and
            // moving down a row only uncovers one new one.  Keep the other two
            // around instead of looking all nine cells up again.
            let mut top = Self::column_sum(prev, left, here, right, stride - 1);
            let mut middle = Self::column_sum(prev, left, here, right, 0);
            let mut bottom_row = 1;
            let mut bottom = Self::column_sum(prev, left, here, right, bottom_row);

            for row in 0..stride {
                let idx = here + row;
                let alive = prev.contains(idx);

                // `middle` counts the cell itself, which is not its own neighbour.
                let neighbours = top + middle + bottom - usize::from(alive);

                next.set(
                    idx,
                    matches!((alive, neighbours), (true, 2 | 3) | (false, 3)),
                );

                top = middle;
                middle = bottom;
                bottom_row += 1;
                if bottom_row == stride {
                    bottom_row = 0;
                }
                bottom = Self::column_sum(prev, left, here, right, bottom_row);
            }

            left = here;
            here = right;
            right += stride;
            if right == size {
                right = 0;
            }
        }

        core::mem::swap(&mut self.prev, &mut self.next);
    }
}

fn run() -> Result<()> {
    let srv = Srv::init()?;

    let mut gpu = Gpu::init(&srv)?;

    use ctru_rt::graphics::FramebufferColorFormat::RGB8;
    let mut gfx = Graphics::init(&mut gpu, RGB8, RGB8, FRAMEBUFFER_ALLOCATOR)?;

    let hid = Hid::init(&srv)?;

    let mut game_bot = GameOfLife::new(gfx.dimensions(Screen::Bottom));
    game_bot.seed(0x1234_5678);
    let mut game_top = GameOfLife::new(gfx.dimensions(Screen::Top));
    game_top.seed(0x8765_4321);

    info!("Press START to exit");
    while !hid.last_keypad().start() {
        gfx.with_pixels_rgb_mut(Screen::Bottom, |pixels| {
            for (idx, pixel) in pixels.iter_mut().enumerate() {
                *pixel = if game_bot.prev.contains(idx) {
                    [0xff; 3]
                } else {
                    [0x00; 3]
                }
            }
        });
        gfx.with_pixels_rgb_mut(Screen::Top, |pixels| {
            for (idx, pixel) in pixels.iter_mut().enumerate() {
                *pixel = if game_top.prev.contains(idx) {
                    [0xff; 3]
                } else {
                    [0x00; 3]
                }
            }
        });

        gfx.swap_buffers()?;
        gfx.wait_vblank0()?;

        game_bot.update();
        game_top.update();
    }

    info!("Exiting...");

    Ok(())
}

#[entry]
fn main() {
    let _ = ctru_rt::debug::init_log();

    match run() {
        Ok(_) => {}
        Err(e) => {
            error!("Failed to run: {:?}", e);
            svc::exit_process()
        }
    }
}
