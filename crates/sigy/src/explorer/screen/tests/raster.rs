//! README hero raster. Cells come from the production renderer.
//!
//! ASCII glyphs are the public-domain `font8x8` basic Latin table by Daniel Hepper,
//! based on Marcel Sondaar's public-domain VGA font. Each row's least significant bit
//! is the leftmost pixel. Rows are doubled so one terminal cell is 8 by 16 pixels.
//! Braille and the live marker are drawn from their code points. This is not a native
//! terminal capture and it does not use the private cell rasterizer.

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::text::Span;
use std::io::{Read, Write};
use std::path::Path;

const FONT: &[u8] = include_bytes!("font8x8_basic.bin");
const CELL_W: u32 = 8;
const CELL_H: u32 = 16;
const MARGIN: u32 = 12;
const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Theme {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rgb(u8, u8, u8);

#[derive(Clone, Copy, Debug)]
struct Palette {
    ansi: [Rgb; 16],
    foreground: Rgb,
    background: Rgb,
}

#[derive(Debug)]
struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: u32,
    height: u32,
    foreground: Rgb,
}

/// # Errors
/// Returns an error when a cell glyph is unsupported or the image cannot be encoded.
pub(super) fn png_from_buffer(buffer: &Buffer, theme: Theme) -> Result<Vec<u8>, String> {
    let columns = u32::from(buffer.area.width);
    let rows = u32::from(buffer.area.height);
    let width = columns
        .checked_mul(CELL_W)
        .and_then(|span| span.checked_add(MARGIN.saturating_mul(2)))
        .ok_or("hero width")?;
    let height = rows
        .checked_mul(CELL_H)
        .and_then(|span| span.checked_add(MARGIN.saturating_mul(2)))
        .ok_or("hero height")?;
    let palette = palette(theme);
    let mut pixels = vec![0u8; pixel_len(width, height)?];
    fill(&mut pixels, palette.background);
    draw_cells(&mut pixels, width, height, buffer, palette)?;
    let bytes = encode_png(width, height, &pixels)?;
    if bytes.len() > MAX_BYTES {
        return Err("hero image byte cap exceeded".into());
    }
    Ok(bytes)
}

/// # Errors
/// Returns an error when either image cannot be written.
pub(super) fn write_pair(buffer: &Buffer, dark: &Path, light: &Path) -> Result<(), String> {
    let dark_bytes = png_from_buffer(buffer, Theme::Dark)?;
    let light_bytes = png_from_buffer(buffer, Theme::Light)?;
    std::fs::write(dark, dark_bytes).map_err(|error| format!("write dark hero: {error}"))?;
    std::fs::write(light, light_bytes).map_err(|error| format!("write light hero: {error}"))?;
    Ok(())
}

#[must_use]
pub(super) fn supported_symbol(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    let Some(ch) = chars.next() else {
        return true;
    };
    chars.next().is_none() && glyph_known(ch)
}

fn glyph_known(ch: char) -> bool {
    ch == '●' || ch == '★' || ('\u{2800}'..='\u{28ff}').contains(&ch) || matches!(ch, ' '..='~')
}

fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Dark => Palette {
            ansi: [
                Rgb(0x0c, 0x0f, 0x12),
                Rgb(0xc5, 0x0f, 0x1f),
                Rgb(0x13, 0xa1, 0x0e),
                Rgb(0xc1, 0x9c, 0x00),
                Rgb(0x00, 0x37, 0xda),
                Rgb(0x88, 0x17, 0x98),
                Rgb(0x3a, 0x96, 0xdd),
                Rgb(0xcc, 0xcc, 0xcc),
                Rgb(0x87, 0x91, 0x9d),
                Rgb(0xe7, 0x48, 0x56),
                Rgb(0x16, 0xc6, 0x0c),
                Rgb(0xf9, 0xf1, 0xa5),
                Rgb(0x3b, 0x78, 0xff),
                Rgb(0xb4, 0x00, 0x9e),
                Rgb(0x61, 0xd6, 0xd6),
                Rgb(0xff, 0xff, 0xff),
            ],
            foreground: Rgb(0xd9, 0xdf, 0xe7),
            background: Rgb(0x0c, 0x0f, 0x12),
        },
        Theme::Light => Palette {
            ansi: [
                Rgb(0x2a, 0x26, 0x1f),
                Rgb(0x9f, 0x1d, 0x1d),
                Rgb(0x1b, 0x7a, 0x32),
                Rgb(0x8a, 0x5a, 0x00),
                Rgb(0x1a, 0x4f, 0x8b),
                Rgb(0x6d, 0x3a, 0x86),
                Rgb(0x0d, 0x6e, 0x78),
                Rgb(0x5e, 0x58, 0x4e),
                Rgb(0x7a, 0x74, 0x6a),
                Rgb(0xc4, 0x45, 0x3c),
                Rgb(0x2d, 0x8a, 0x40),
                Rgb(0xa6, 0x7c, 0x00),
                Rgb(0x2b, 0x62, 0xb0),
                Rgb(0x8d, 0x4e, 0x9c),
                Rgb(0x1a, 0x8c, 0x93),
                Rgb(0x21, 0x1e, 0x18),
            ],
            foreground: Rgb(0x21, 0x1e, 0x18),
            background: Rgb(0xf4, 0xf0, 0xe6),
        },
    }
}

fn fill(pixels: &mut [u8], color: Rgb) {
    for chunk in pixels.as_chunks_mut::<3>().0 {
        *chunk = [color.0, color.1, color.2];
    }
}

fn draw_cells(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    buffer: &Buffer,
    palette: Palette,
) -> Result<(), String> {
    let columns = buffer.area.width;
    let rows = buffer.area.height;
    for y in 0..rows {
        let mut until = 0u16;
        for x in 0..columns {
            if x < until {
                continue;
            }
            let cell = &buffer[(x, y)];
            if !supported_symbol(cell.symbol()) {
                return Err(format!("unsupported hero glyph {:?}", cell.symbol()));
            }
            let span = Span::raw(cell.symbol()).width().max(1);
            until = x.saturating_add(u16::try_from(span).unwrap_or(1));
            let mut foreground = map_color(palette, cell.fg, false);
            let mut background = map_color(palette, cell.bg, true);
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut foreground, &mut background);
            }
            let origin_x = MARGIN + u32::from(x) * CELL_W;
            let origin_y = MARGIN + u32::from(y) * CELL_H;
            let mut canvas = Canvas {
                pixels,
                width,
                height,
                foreground,
            };
            paint_rect(&mut canvas, origin_x, origin_y, CELL_W, CELL_H, background);
            if cell.modifier.contains(Modifier::HIDDEN) {
                continue;
            }
            draw_symbol(
                &mut canvas,
                origin_x,
                origin_y,
                cell.symbol(),
                cell.modifier.contains(Modifier::BOLD),
            );
            if cell.modifier.contains(Modifier::UNDERLINED) {
                paint_rect(
                    &mut canvas,
                    origin_x,
                    origin_y + CELL_H - 1,
                    CELL_W,
                    1,
                    foreground,
                );
            }
        }
    }
    Ok(())
}

fn draw_symbol(canvas: &mut Canvas<'_>, x: u32, y: u32, symbol: &str, bold: bool) {
    let Some(ch) = symbol.chars().next() else {
        return;
    };
    match ch {
        '●' => draw_disc(canvas, x, y),
        '★' => draw_star(canvas, x, y),
        ch if ('\u{2800}'..='\u{28ff}').contains(&ch) => {
            let bits = u32::from(ch) - 0x2800;
            draw_braille(canvas, x, y, u8::try_from(bits).unwrap_or(0));
        }
        ch if matches!(ch, ' '..='~') => {
            draw_ascii(
                canvas,
                x,
                y,
                u8::try_from(u32::from(ch)).unwrap_or(b' '),
                bold,
            );
        }
        _ => {}
    }
}

fn draw_ascii(canvas: &mut Canvas<'_>, x: u32, y: u32, ch: u8, bold: bool) {
    let Some(rows) = glyph_rows(ch) else {
        return;
    };
    for (row, bits) in rows.into_iter().enumerate() {
        for col in 0..8u32 {
            let shift = u8::try_from(col).unwrap_or(0);
            if bits & (1u8 << shift) == 0 {
                continue;
            }
            let py = y + u32::try_from(row).unwrap_or(0) * 2;
            stamp(canvas, x + col, py);
            stamp(canvas, x + col, py + 1);
            if bold && col + 1 < CELL_W {
                stamp(canvas, x + col + 1, py);
                stamp(canvas, x + col + 1, py + 1);
            }
        }
    }
}

fn glyph_rows(ch: u8) -> Option<[u8; 8]> {
    let start = usize::from(ch).checked_mul(8)?;
    let bytes = FONT.get(start..start.checked_add(8)?)?;
    let mut rows = [0u8; 8];
    rows.copy_from_slice(bytes);
    Some(rows)
}

fn draw_braille(canvas: &mut Canvas<'_>, x: u32, y: u32, bits: u8) {
    const DOTS: [(u8, u32, u32); 8] = [
        (0x01, 1, 1),
        (0x02, 1, 5),
        (0x04, 1, 9),
        (0x40, 1, 13),
        (0x08, 4, 1),
        (0x10, 4, 5),
        (0x20, 4, 9),
        (0x80, 4, 13),
    ];
    let color = canvas.foreground;
    for (mask, dx, dy) in DOTS {
        if bits & mask == 0 {
            continue;
        }
        paint_rect(canvas, x + dx, y + dy, 2, 2, color);
    }
}

fn draw_disc(canvas: &mut Canvas<'_>, x: u32, y: u32) {
    for dy in 0..CELL_H {
        for dx in 0..CELL_W {
            let Ok(px) = i32::try_from(dx) else {
                continue;
            };
            let Ok(py) = i32::try_from(dy) else {
                continue;
            };
            let px = px - 3;
            let py = py - 7;
            if px * px + py * py <= 16 {
                stamp(canvas, x + dx, y + dy);
            }
        }
    }
}

fn draw_star(canvas: &mut Canvas<'_>, x: u32, y: u32) {
    const POINTS: [(u32, u32); 12] = [
        (3, 1),
        (3, 2),
        (3, 3),
        (3, 4),
        (3, 5),
        (3, 6),
        (1, 3),
        (2, 3),
        (4, 3),
        (5, 3),
        (2, 5),
        (4, 5),
    ];
    for (dx, dy) in POINTS {
        stamp(canvas, x + dx, y + dy);
    }
}

fn paint_rect(canvas: &mut Canvas<'_>, x: u32, y: u32, w: u32, h: u32, color: Rgb) {
    for dy in 0..h {
        for dx in 0..w {
            put(canvas, x + dx, y + dy, color);
        }
    }
}

fn stamp(canvas: &mut Canvas<'_>, x: u32, y: u32) {
    let color = canvas.foreground;
    put(canvas, x, y, color);
}

fn put(canvas: &mut Canvas<'_>, x: u32, y: u32, color: Rgb) {
    if x >= canvas.width || y >= canvas.height {
        return;
    }
    let Some(offset) = y
        .checked_mul(canvas.width)
        .and_then(|index| index.checked_add(x))
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| index.checked_mul(3))
    else {
        return;
    };
    if let Some(slot) = canvas.pixels.get_mut(offset..offset + 3) {
        slot.copy_from_slice(&[color.0, color.1, color.2]);
    }
}

fn map_color(palette: Palette, color: Color, background: bool) -> Rgb {
    if let Some(index) = named_slot(color) {
        return palette.ansi[usize::from(index)];
    }
    match color {
        Color::Rgb(red, green, blue) => Rgb(red, green, blue),
        Color::Indexed(index) => indexed(palette, index),
        _ => {
            if background {
                palette.background
            } else {
                palette.foreground
            }
        }
    }
}

fn pixel_len(width: u32, height: u32) -> Result<usize, &'static str> {
    let width = usize::try_from(width).map_err(|_| "hero width")?;
    let height = usize::try_from(height).map_err(|_| "hero height")?;
    width
        .checked_mul(height)
        .and_then(|cells| cells.checked_mul(3))
        .ok_or("hero pixels")
}

fn named_slot(color: Color) -> Option<u8> {
    Some(match color {
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        _ => return None,
    })
}

fn indexed(palette: Palette, index: u8) -> Rgb {
    if index < 16 {
        return palette.ansi[usize::from(index)];
    }
    if index >= 232 {
        let level = 8u16 + 10u16 * u16::from(index - 232);
        let gray = u8::try_from(level).unwrap_or(255);
        return Rgb(gray, gray, gray);
    }
    let cube = index - 16;
    let levels = [0u8, 95, 135, 175, 215, 255];
    Rgb(
        levels[usize::from(cube / 36)],
        levels[usize::from((cube / 6) % 6)],
        levels[usize::from(cube % 6)],
    )
}

/// # Errors
/// Returns an error when the image dimensions or compressed data do not fit a PNG chunk.
fn encode_png(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let row = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(3))
        .ok_or("png row")?;
    let rows = usize::try_from(height).map_err(|_| "png height")?;
    let mut raw = Vec::new();
    let stride = row.checked_add(1).ok_or("png stride")?;
    raw.try_reserve_exact(stride.checked_mul(rows).ok_or("png raw")?)
        .map_err(|_| "png reserve")?;
    for y in 0..rows {
        raw.push(0);
        let start = y.checked_mul(row).ok_or("png row start")?;
        let end = start.checked_add(row).ok_or("png row end")?;
        raw.extend_from_slice(rgb.get(start..end).ok_or("png rgb")?);
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(9));
    encoder
        .write_all(&raw)
        .map_err(|error| format!("png deflate: {error}"))?;
    let compressed = encoder
        .finish()
        .map_err(|error| format!("png deflate finish: {error}"))?;
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    push_chunk(&mut png, *b"IHDR", &ihdr)?;
    push_chunk(&mut png, *b"IDAT", &compressed)?;
    push_chunk(&mut png, *b"IEND", &[])?;
    Ok(png)
}

fn push_chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) -> Result<(), String> {
    let len = u32::try_from(data.len()).map_err(|_| "png chunk length")?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(&kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    Ok(())
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn decode_rgb(png: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if png.len() < 8 || &png[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("png signature".into());
    }
    let mut cursor = 8usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut compressed = Vec::new();
    while cursor + 8 <= png.len() {
        let len = u32::from_be_bytes(
            png.get(cursor..cursor + 4)
                .ok_or("png length")?
                .try_into()
                .map_err(|_| "png length")?,
        );
        let kind = png.get(cursor + 4..cursor + 8).ok_or("png kind")?;
        let start = cursor + 8;
        let size = usize::try_from(len).map_err(|_| "png size")?;
        let data = png.get(start..start + size).ok_or("png data")?;
        if kind == b"IHDR" && data.len() >= 8 {
            width = u32::from_be_bytes(data[0..4].try_into().map_err(|_| "png width")?);
            height = u32::from_be_bytes(data[4..8].try_into().map_err(|_| "png height")?);
        } else if kind == b"IDAT" {
            compressed.extend_from_slice(data);
        }
        cursor = start + size + 4;
    }
    let mut decoder = ZlibDecoder::new(compressed.as_slice());
    let mut raw = Vec::new();
    decoder
        .read_to_end(&mut raw)
        .map_err(|error| format!("png inflate: {error}"))?;
    let row = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(3))
        .ok_or("png decoded row")?;
    let mut rgb = Vec::new();
    let mut offset = 0usize;
    for _ in 0..height {
        if raw.get(offset) != Some(&0) {
            return Err("png filter".into());
        }
        offset += 1;
        rgb.extend_from_slice(raw.get(offset..offset + row).ok_or("png decoded")?);
        offset += row;
    }
    Ok((width, height, rgb))
}

#[cfg(test)]
mod tests {
    use super::{FONT, MAX_BYTES, Theme, decode_rgb, png_from_buffer, supported_symbol};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    #[test]
    fn font_table_covers_basic_latin_and_the_exclamation_is_centered() {
        assert_eq!(FONT.len(), 1024);
        let rows = super::glyph_rows(b'!').unwrap_or([0; 8]);
        assert_eq!(rows[0], 0x18);
    }

    #[test]
    fn supported_symbols_are_the_hero_repertoire() {
        assert!(supported_symbol(""));
        assert!(supported_symbol("A"));
        assert!(supported_symbol(" "));
        assert!(supported_symbol("\u{2801}"));
        assert!(supported_symbol("●"));
        assert!(supported_symbol("★"));
        assert!(!supported_symbol("東京"));
        assert!(!supported_symbol("\n"));
    }

    #[test]
    fn one_green_cell_round_trips_and_themes_differ() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer[(0, 0)].set_symbol("A");
        buffer[(0, 0)].fg = Color::Green;
        let dark = png_from_buffer(&buffer, Theme::Dark).unwrap_or_else(|error| panic!("{error}"));
        let again = png_from_buffer(&buffer, Theme::Dark).unwrap_or_else(|error| panic!("{error}"));
        let light =
            png_from_buffer(&buffer, Theme::Light).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(dark, again);
        assert_ne!(dark, light);
        assert!(dark.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(dark.len() < MAX_BYTES);
        let (width, height, rgb) = decode_rgb(&dark).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((width, height), (32, 40));
        assert_eq!(&rgb[0..3], &[0x0c, 0x0f, 0x12]);
        assert!(rgb.as_chunks::<3>().0.contains(&[0x13, 0xa1, 0x0e]));
    }

    #[test]
    fn braille_dot_one_is_a_small_mark_not_a_filled_cell() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer[(0, 0)].set_symbol("\u{2801}");
        buffer[(0, 0)].fg = Color::Green;
        let png = png_from_buffer(&buffer, Theme::Dark).unwrap_or_else(|error| panic!("{error}"));
        let (_, _, rgb) = decode_rgb(&png).unwrap_or_else(|error| panic!("{error}"));
        let green = rgb
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|pixel| **pixel == [0x13, 0xa1, 0x0e])
            .count();
        assert_eq!(green, 4);
    }

    #[test]
    fn live_marker_fills_its_cell_without_becoming_a_block() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer[(0, 0)].set_symbol("●");
        buffer[(0, 0)].fg = Color::Green;
        let png = png_from_buffer(&buffer, Theme::Dark).unwrap_or_else(|error| panic!("{error}"));
        let (_, _, rgb) = decode_rgb(&png).unwrap_or_else(|error| panic!("{error}"));
        let green = rgb
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|pixel| **pixel == [0x13, 0xa1, 0x0e])
            .count();
        assert!((40..=80).contains(&green), "{green}");
    }
}
