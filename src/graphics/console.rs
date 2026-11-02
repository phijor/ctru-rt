// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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
}
