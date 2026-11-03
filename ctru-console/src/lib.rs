// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

extern crate alloc;

use alloc::{boxed::Box, vec};
use core::ops::Range;

use ctru_rt::{graphics::Graphics, services::gsp::gpu::Screen};

/// `build.rs` generates a binary blob containing glyphs extracted from a BDF font.
/// The blob consists of a 4 byte header, followed by an array of glyph bitmaps.
/// The array holds bitmaps for a single consecutive subrange of ASCII characters.
///
/// The header ([`HEADER`]) consists of the following bytes, in order:
/// * [`ADVANCE`]: The horizontal distance in pixels from one glyph's origin to the next.
///   This includes spacing between glyphs. If, for example, the widest glyph measures 6
///   pixels horizontally, then `ADVANCE` may be 7, as adjacent glyphs would otherwise merge.
///   This corresponds to `DWIDTH` in the original BDF font.
/// * [`HEIGHT`]: The height of the font bounding box in pixels.
///   Each glyph bitmap is padded to this height, even for glyphs that have a smaller bounding box.
/// * [`FIRST`]: The ASCII code for the first character present in the font.
/// * [`COUNT`]: The number of characters in the font. In total the range `(FIRST..FIRST+COUNT)` of
///   characters is provided by the font.
///
/// The header is followed by an array of bitmaps ([`GLYPHS`]) of length `COUNT + 1`:
/// one glyph for each encoded character, plus one fallback character.
///
/// The bitmap for a single glyph is simply an array of bytes, `[u8; HEIGHT]`:
/// one row of pixels is encoded per `u8`, and there are `HEIGHT` rows per glyph.
/// See [`Glyph`] for the exact bit-level layout.
const FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/font.bin"));

const HEADER: &[u8] = FONT.split_at(4).0;

const RAW_GLYPHS: &[u8] = FONT.split_at(4).1;

const ADVANCE: usize = HEADER[0] as usize;

const HEIGHT: usize = HEADER[1] as usize;

const FIRST: usize = HEADER[2] as usize;

const COUNT: usize = HEADER[3] as usize;

type Row = u8;

const GLYPHS: &[[Row; HEIGHT]] = RAW_GLYPHS.as_chunks::<HEIGHT>().0;

const _: () = assert!(RAW_GLYPHS.as_chunks::<HEIGHT>().1.is_empty());

const _: () = assert!(GLYPHS.len() == COUNT + 1);

/// A reference to a glyph bitmap in the font.
///
/// Points to an array `[Row; HEIGHT]` where `Row = u8`:
/// one row of pixels is encoded per `u8`, and there are `HEIGHT` rows per glyph.
/// Rows are encoded top-to-bottom. In a row, the most significant bit encodes the leftmost pixel.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Glyph(&'static [Row; HEIGHT]);

impl Glyph {
    /// The fallback glyph used for characters that have no glyph in the font.
    pub(crate) const fn fallback() -> Self {
        Self(&GLYPHS[COUNT])
    }

    /// Get the glyph bitmap for a Unicode character.
    ///
    /// If the character does not have a glyph in the font, returns [`Self::fallback`].
    pub(crate) const fn at(c: char) -> Self {
        let Some(index) = (c as usize).checked_sub(FIRST) else {
            return Self::fallback();
        };

        if index < COUNT {
            Self(&GLYPHS[index])
        } else {
            Self::fallback()
        }
    }

    pub(crate) fn pixel(&self, dx: usize, dy: usize) -> bool {
        dx < (u8::BITS as usize) // Ensure pixel is within column
            && (self.0[dy] & (0b1000_0000 >> dx) != 0)
    }
}

#[derive(Debug)]
pub struct Console {
    cells: Box<[char]>,
    top_line: usize,
    cursor_line: usize,
    cursor_col: usize,
    screen: Screen,
    width: usize,
    height: usize,
}

impl Console {
    const fn layout_for_screen(screen: Screen) -> (usize, usize) {
        let dimensions = screen.dimensions();
        let width = (dimensions.height as usize) / ADVANCE;
        let height = (dimensions.width as usize) / HEIGHT;
        (width, height)
    }

    pub fn new(screen: Screen) -> Self {
        let (width, height) = Self::layout_for_screen(screen);
        debug_assert_ne!(width, 0);
        debug_assert_ne!(height, 0);
        let cells = vec![' '; width * height].into_boxed_slice();

        Self {
            cells,
            screen,
            width,
            height,
            top_line: 0,
            cursor_line: 0,
            cursor_col: 0,
        }
    }

    fn line_range(&self, line: usize) -> Range<usize> {
        let start = line * self.width;
        let end = start + self.width;
        start..end
    }

    fn line_at(&self, line: usize) -> &[char] {
        let range = self.line_range(line);
        &self.cells[range]
    }

    fn line_at_mut(&mut self, line: usize) -> &mut [char] {
        let range = self.line_range(line);
        &mut self.cells[range]
    }

    pub fn char_at(&self, line: usize, col: usize) -> char {
        self.line_at(line)[col]
    }

    fn char_at_mut(&mut self, line: usize, col: usize) -> &mut char {
        &mut self.line_at_mut(line)[col]
    }

    fn clear_line(&mut self, line: usize) {
        self.line_at_mut(line).fill(' ');
    }

    #[inline]
    fn next_line(&self, mut line: usize) -> usize {
        line += 1;
        if line >= self.height { 0 } else { line }
    }

    fn shift_up(&mut self) {
        self.top_line = self.next_line(self.top_line);
    }

    fn advance_line(&mut self) {
        self.cursor_col = 0;
        self.cursor_line = self.next_line(self.cursor_line);
        if self.cursor_line == self.top_line {
            self.shift_up();
            self.clear_line(self.cursor_line);
        }
    }

    pub fn char_visible_at(&self, line: usize, col: usize) -> char {
        let mut phys_line = self.top_line + line;
        while phys_line >= self.height {
            phys_line -= self.height;
        }

        self.char_at(phys_line, col)
    }

    fn set_char(&mut self, c: char) {
        // Deferred wrap: The cursor points past the end of the current line.
        // Advance to the next line; this also resets the cursor to the first column.
        if self.cursor_col == self.width {
            self.advance_line();
        }

        *self.char_at_mut(self.cursor_line, self.cursor_col) = c;

        // This may point past the end of the current line.
        self.cursor_col += 1;
    }

    fn next_tabstop(&self, col: usize) -> usize {
        const TAB_WIDTH: usize = 4;
        const _: () = assert!(TAB_WIDTH.is_power_of_two());
        // Calculate the next tabstop by rounding down to a multiple of TAB_WIDTH, then adding TAB_WIDTH.
        let tabstop = (col & !(TAB_WIDTH - 1)) + TAB_WIDTH;
        self.width.min(tabstop)
    }

    pub fn put_char(&mut self, c: char) {
        match c {
            '\n' => self.advance_line(),
            '\r' => self.cursor_col = 0,
            '\t' => self.cursor_col = self.next_tabstop(self.cursor_col),
            c => self.set_char(c),
        }
    }

    pub fn put_str(&mut self, s: &str) {
        for c in s.chars() {
            self.put_char(c)
        }
    }

    pub fn put_strln(&mut self, s: &str) {
        self.put_str(s);
        self.advance_line();
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn render(&self, gfx: &mut Graphics<'_>) {
        let dim = self.screen.dimensions();
        const FG: [u8; 3] = [0xff; 3];
        const BG: [u8; 3] = [0x00; 3];
        gfx.with_pixels_rgb_mut(self.screen, |pixels| {
            for (pixel_x, screen_column) in
                pixels.chunks_exact_mut(usize::from(dim.width)).enumerate()
            {
                // The pixel lies at some horizontal offset `dx` in a character at `column`.
                let (column, dx) = (pixel_x / ADVANCE, pixel_x % ADVANCE);

                if column >= self.width {
                    screen_column.fill(BG);
                    continue;
                }

                // Columns run bottom-to-top. Cut them into chunks of `HEIGHT` pixels, one
                // chunk per character, one pixel per character row.
                let (rem, chunks) = screen_column.as_rchunks_mut::<HEIGHT>();

                // The remainder at the bottom of the screen needs to be filled.
                rem.fill(BG);

                // The chunks come in memory order (bottom-to-top), but lines run top-to-bottom.
                // Iterate over the chunks in reverse.
                for (line, cell) in chunks.iter_mut().rev().enumerate() {
                    let glyph = Glyph::at(self.char_visible_at(line, column));
                    for (dy, pixel) in cell.iter_mut().rev().enumerate() {
                        *pixel = if glyph.pixel(dx, dy) { FG } else { BG };
                    }
                }
            }
        });
    }
}

impl core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.put_str(s);
        Ok(())
    }
}
