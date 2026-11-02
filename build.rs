// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// NOTE: Adapted from cortex-m/build.rs

use std::env;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use bdf_reader::{BoundingBox, Font, Glyph};

/// Font the console renders with.
const FONT_PATH: &str = "assets/Bonto-Condensed.bdf";

/// First codepoint included in the font blob.
const FIRST: u8 = 0x20;

/// Last codepoint included in the font blob.
const LAST: u8 = 0x7E;

/// Set this to print some glyphs of the encoded font as build warnings.
const DEBUG_FONT_VAR: &str = "CTRU_RT_DEBUG_FONT";

fn main() -> Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={DEBUG_FONT_VAR}");

    copy_linker_script(&out_dir)?;
    let font = parse_font()?;

    let blob = encode_font(&font)?;
    fs::write(out_dir.join("font.bin"), &blob).context("Failed to write font blob")?;

    if env::var_os(DEBUG_FONT_VAR).is_some() {
        debug_font(&blob)?;
    }

    Ok(())
}

/// Put the linker script somewhere the linker can find it
fn copy_linker_script(out_dir: &Path) -> Result<()> {
    let script = PathBuf::from("link.x");

    fs::copy(&script, out_dir.join(script.file_name().unwrap()))
        .context("Failed to copy linker script")?;
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rustc-link-search={}", out_dir.display());

    Ok(())
}

fn parse_font() -> Result<Font> {
    println!("cargo:rerun-if-changed={FONT_PATH}");
    let file = File::open(FONT_PATH).with_context(|| format!("Failed to open {FONT_PATH}"))?;
    Font::read(BufReader::new(file)).context("Failed to parse font")
}

/// Encode the glyphs for `FIRST..=LAST` into the blob the console loads.
///
/// The blob starts with a header `[advance, height, first, count]`, followed by `count` glyphs
/// for the codepoints starting at `first`, followed by one fallback glyph for characters missing
/// from the font.  Each glyph is `height` bytes, one per row of its cell, top row first.  The
/// most significant bit of each byte is the leftmost pixel.
fn encode_font(font: &Font) -> Result<Vec<u8>> {
    let bbox = font.bounding_box();
    let advance = advance(font)?;
    let height = u8::try_from(bbox.height).context("Font is too tall")?;
    // Rows of the cell above the baseline.
    let ascent = (bbox.height as i32 + bbox.offset_y).clamp(0, i32::from(height)) as u8;

    let fallback = fallback_glyph(advance, height, ascent);

    let mut blob = vec![advance, height, FIRST, LAST - FIRST + 1];
    for c in FIRST..=LAST {
        match font.glyph(u32::from(c)) {
            Some(glyph) => blob.extend(place_glyph(bbox, advance, glyph)?),
            None => {
                println!(
                    "cargo:warning=Font lacks {:?}, using fallback glyph",
                    char::from(c)
                );
                blob.extend(&fallback);
            }
        }
    }
    blob.extend(&fallback);

    Ok(blob)
}

/// The horizontal advance shared by all glyphs in `FIRST..=LAST`.
///
/// Fails if the font is not monospace.
fn advance(font: &Font) -> Result<u8> {
    let mut advance = None;

    for c in FIRST..=LAST {
        let Some(glyph) = font.glyph(u32::from(c)) else {
            continue;
        };
        let (dx, dy) = glyph
            .dwidth()
            .with_context(|| format!("Glyph {} has no DWIDTH", glyph.name()))?;
        ensure!(
            dy == 0.0 && dx.fract() == 0.0 && (1.0..=255.0).contains(&dx),
            "Glyph {} has unsupported DWIDTH {dx} {dy}",
            glyph.name()
        );
        let dx = dx as u8;

        match advance {
            None => advance = Some(dx),
            Some(advance) => ensure!(
                advance == dx,
                "Font is not monospace: glyph {} advances {dx} pixels instead of {advance}",
                glyph.name()
            ),
        }
    }

    advance.context("Font has no printable ASCII glyphs")
}

/// Place `glyph` in a cell spanning the font bounding box `font_bbox`.
fn place_glyph(font_bbox: BoundingBox, advance: u8, glyph: &Glyph) -> Result<Vec<u8>> {
    let bbox = glyph.bounding_box();
    let bitmap = glyph.bitmap();

    // Both boxes are positioned by their bottom left corner, relative to the glyph origin on the
    // baseline.  Rows are counted from the top of the cell, though.
    let cell_height = font_bbox.height as i32;
    let top = cell_height + font_bbox.offset_y - (bbox.height as i32 + bbox.offset_y);
    let shift = bbox.offset_x - font_bbox.offset_x;

    ensure!(
        top >= 0 && top + bbox.height as i32 <= cell_height,
        "Glyph {} sticks out of the font bounding box vertically",
        glyph.name()
    );
    let width = i32::from(advance.min(8));
    ensure!(
        shift >= 0 && shift + bbox.width as i32 <= width,
        "Glyph {} does not fit into {width} pixel wide rows",
        glyph.name()
    );
    let (top, shift) = (top as usize, shift as usize);

    let mut cell = vec![0; font_bbox.height as usize];
    for y in 0..bitmap.height() {
        for x in 0..bitmap.width() {
            if bitmap.get(x, y)? {
                cell[top + y] |= 0x80 >> (shift + x);
            }
        }
    }

    Ok(cell)
}

/// A hollow box filling the part of a cell above the baseline, for characters missing from the
/// font.
fn fallback_glyph(advance: u8, height: u8, ascent: u8) -> Vec<u8> {
    // Leave the last column blank, to keep the box apart from the next glyph.
    let width = advance.saturating_sub(1).min(8);

    let mut cell = vec![0; usize::from(height)];
    for y in 0..ascent {
        for x in 0..width {
            if y == 0 || y == ascent - 1 || x == 0 || x == width - 1 {
                cell[usize::from(y)] |= 0x80 >> x;
            }
        }
    }

    cell
}

/// Print some glyphs of the encoded font `blob` as build warnings.
///
/// This decodes the blob rather than using the parsed font, so it checks the encoding as well.
fn debug_font(blob: &[u8]) -> Result<()> {
    let &[advance, height, first, count, ref glyphs @ ..] = blob else {
        bail!("Font blob lacks a header");
    };
    let cell = |index: u8| -> Result<&[u8]> {
        glyphs
            .chunks(usize::from(height))
            .nth(usize::from(index))
            .with_context(|| format!("Glyph at index {index} not present in font"))
    };

    for c in ['A', 'g', '|'] {
        println!("cargo:warning={c:?}:");
        debug_cell(advance, cell(c as u8 - first)?);
    }
    println!("cargo:warning=fallback:");
    debug_cell(advance, cell(count)?);

    Ok(())
}

fn debug_cell(advance: u8, cell: &[u8]) {
    for row in cell {
        let line: String = (0..advance)
            .map(|x| {
                if x < 8 && row & (0x80 >> x) != 0 {
                    '#'
                } else {
                    '.'
                }
            })
            .collect();
        println!("cargo:warning={line}");
    }
}
