// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

extern crate alloc;

use core::{fmt::Write, panic::PanicInfo};

use alloc::{boxed::Box, vec};

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

/// One word of cell storage, holding `WORD_BITS` cells.
type Word = usize;

/// Bits in one word of the cell storage.
const WORD_BITS: usize = Word::BITS as usize;

/// Cells simulated per framebuffer column.
///
/// The screens are 240 pixels wide, but rounding the column up to a whole number of
/// words is what makes the bit-parallel update possible: the cells of two adjacent
/// columns then line up bit for bit, so a word of one column can be combined with the
/// word next to it without shifting anything into place first.
const COLUMN_BITS: usize = 256;

/// Words of cell storage per framebuffer column.
const WORDS_PER_COLUMN: usize = COLUMN_BITS / WORD_BITS;

/// One column of cells, to be read as `WORD_BITS` cells per word, least significant
/// bit first.
type Column = [Word; WORDS_PER_COLUMN];

/// Generations to hold the visible window still before scrolling it down one row.
/// A full cycle takes `COLUMN_BITS * GENERATIONS_PER_ROW` generations, about eight
/// seconds at sixty frames per second.
const GENERATIONS_PER_ROW: usize = 2;

struct GameOfLife {
    next: Box<[Word]>,
    prev: Box<[Word]>,
    /// Number of cells of a column that fit on screen.
    visible_rows: usize,
    /// Number of framebuffer columns.
    columns: usize,
    /// First cell of a column to draw.  The rows past the edge of the screen are not
    /// wasted: this creeps along, scrolling them into view.
    offset: usize,
    /// Generations drawn at the current [`Self::offset`].
    held: usize,
}

impl GameOfLife {
    fn new(dimensions: &ScreenDimensions) -> Self {
        // Framebuffers are stored column-major: `width` pixels make up one column, of
        // which there are `height`.
        let visible_rows = usize::from(dimensions.width);
        let columns = usize::from(dimensions.height);
        debug_assert!(visible_rows <= COLUMN_BITS);

        let words = WORDS_PER_COLUMN * columns;

        Self {
            next: vec![0; words].into_boxed_slice(),
            prev: vec![0; words].into_boxed_slice(),
            visible_rows,
            columns,
            offset: 0,
            held: 0,
        }
    }

    fn seed(&mut self, seed: u32) {
        let mut rng = seed | 1;

        for word in &mut self.prev {
            let mut bits: Word = 0;

            for _ in 0..WORD_BITS / u32::BITS as usize {
                // Xorshift rather than an LCG: every bit of the result is usable, so
                // one draw fills 32 cells instead of one.
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;

                bits = bits.rotate_left(u32::BITS) | rng as Word;
            }

            *word = bits;
        }
    }

    #[inline]
    fn column(cells: &[Word], column: usize) -> &Column {
        let base = column * WORDS_PER_COLUMN;
        cells[base..base + WORDS_PER_COLUMN]
            .try_into()
            .expect("slice spans exactly one column")
    }

    /// The cells one row above, at, and one row below every cell of word `word` of
    /// `column`, wrapping around the ends of the column.
    #[inline]
    fn rows(column: &Column, word: usize) -> (Word, Word, Word) {
        let here = column[word];
        let above = column[(word + WORDS_PER_COLUMN - 1) % WORDS_PER_COLUMN];
        let below = column[(word + 1) % WORDS_PER_COLUMN];

        (
            (here << 1) | (above >> (WORD_BITS - 1)),
            here,
            (here >> 1) | (below << (WORD_BITS - 1)),
        )
    }

    /// Add three cells per bit position, as a two bit number spread over two words.
    #[inline]
    fn add3(a: Word, b: Word, c: Word) -> (Word, Word) {
        let partial = a ^ b;

        (partial ^ c, (a & b) | (partial & c))
    }

    fn update(&mut self) {
        let columns = self.columns;
        let prev: &[Word] = &self.prev;
        let next: &mut [Word] = &mut self.next;

        // Walk the index of the column left of, at, and right of the one being
        // updated, wrapping around the edges of the screen.
        let mut left = columns - 1;
        let mut here = 0;
        let mut right = 1;

        for _ in 0..columns {
            let (l, h, r) = (
                Self::column(prev, left),
                Self::column(prev, here),
                Self::column(prev, right),
            );
            let updated: &mut Column = {
                let base = here * WORDS_PER_COLUMN;
                (&mut next[base..base + WORDS_PER_COLUMN])
                    .try_into()
                    .expect("slice spans exactly one column")
            };

            for (word, out) in updated.iter_mut().enumerate() {
                let (left_up, left_here, left_down) = Self::rows(l, word);
                let (up, alive, down) = Self::rows(h, word);
                let (right_up, right_here, right_down) = Self::rows(r, word);

                // Sum each of the three columns of the neighbourhood on its own, as a
                // two bit number per cell.  The middle one counts the cell itself.
                let (l0, l1) = Self::add3(left_up, left_here, left_down);
                let (h0, h1) = Self::add3(up, alive, down);
                let (r0, r1) = Self::add3(right_up, right_here, right_down);

                // Add those three up into a four bit number per cell, 0..=9.
                let low = l0 ^ h0 ^ r0;
                let low_carry = (l0 & h0) | (l0 & r0) | (h0 & r0);

                let high = l1 ^ h1 ^ r1;
                let high_carry = (l1 & h1) | (l1 & r1) | (h1 & r1);

                let (second, second_carry) = (high ^ low_carry, high & low_carry);
                let third = high_carry ^ second_carry;
                let fourth = high_carry & second_carry;

                // Because the total counts the cell itself, a cell is alive in the
                // next generation if the total is three, or if it is four and the cell
                // is alive now.
                let three = !fourth & !third & second & low;
                let four = !fourth & third & !second & !low;

                *out = three | (alive & four);
            }

            left = here;
            here = right;
            right += 1;
            if right == columns {
                right = 0;
            }
        }

        core::mem::swap(&mut self.prev, &mut self.next);

        // Creep down the board, so that the cells simulated past the edge of the
        // screen eventually show up.
        self.held += 1;
        if self.held == GENERATIONS_PER_ROW {
            self.held = 0;
            self.offset = (self.offset + 1) % COLUMN_BITS;
        }
    }

    /// Paint the visible window of the board into a framebuffer.
    fn draw(&self, pixels: &mut [[u8; 3]]) {
        let cells: &[Word] = &self.prev;

        for (column, pixels) in pixels.chunks_mut(self.visible_rows).enumerate() {
            let column = Self::column(cells, column);

            for (row, pixel) in pixels.iter_mut().enumerate() {
                let cell = (row + self.offset) % COLUMN_BITS;
                let alive = (column[cell / WORD_BITS] >> (cell % WORD_BITS)) & 1;

                *pixel = [(alive as u8).wrapping_neg(); 3];
            }
        }
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
        gfx.with_pixels_rgb_mut(Screen::Bottom, |pixels| game_bot.draw(pixels));
        gfx.with_pixels_rgb_mut(Screen::Top, |pixels| game_top.draw(pixels));

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
