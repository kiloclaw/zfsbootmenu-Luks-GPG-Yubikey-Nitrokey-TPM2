//! PNG files of drawn frames: the console's screenshots, and the reference
//! images of the snapshot tests.
//!
//! The images are RGB, 8 bits a channel, without filters, compressed with
//! `miniz_oxide` at a fixed level: the same pixels always give the same
//! bytes, so a snapshot compares pixels by comparing files, and any image
//! viewer shows one. Screenshots are compressed at the fastest level, so that
//! taking one does not hold up the console, and the copies of the screen the
//! encoder makes are wiped before they are freed.

use crate::frame::{Frame, Rgb};

/// The compression level of reference images: the best, as they are kept.
const LEVEL: u8 = 9;

/// The compression level of screenshots: the fastest, as the console waits.
const FAST: u8 = 1;

/// The CRC-32 of `bytes` (ISO 3309), which closes each chunk.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

/// Appends the chunk of `kind` holding `data` to `out`.
fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&length.to_be_bytes());
    let start = out.len();
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let crc = crc32(out.get(start..).unwrap_or_default());
    out.extend_from_slice(&crc.to_be_bytes());
}

/// The PNG file of an image `width` pixels wide of `pixels`, row after
/// row; a last row that is not whole is left out.
pub fn png(width: u32, pixels: &[Rgb]) -> Vec<u8> {
    encode(width, pixels, LEVEL)
}

/// [`png`] at `level`; the raw rows are wiped once compressed, and the file
/// is made in one allocation.
fn encode(width: u32, pixels: &[Rgb], level: u8) -> Vec<u8> {
    let columns = usize::try_from(width).unwrap_or(0).max(1);
    let rows = pixels.chunks_exact(columns);
    let height = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let mut raw = Vec::with_capacity(rows.len() * (1 + 3 * columns));
    for row in rows {
        // Filter type 0: the bytes as they are.
        raw.push(0);
        for &Rgb(r, g, b) in row {
            raw.extend_from_slice(&[r, g, b]);
        }
    }
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // 8 bits a channel, colour type 2 (RGB), deflate, no filter choice, no
    // interlace.
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut data = miniz_oxide::deflate::compress_to_vec_zlib(&raw, level);
    zbm_sys::wipe_vec(&mut raw);
    // The signature, then three chunks of 12 bytes each beside their data.
    let mut out = Vec::with_capacity(8 + 3 * 12 + header.len() + data.len());
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    chunk(&mut out, *b"IHDR", &header);
    chunk(&mut out, *b"IDAT", &data);
    chunk(&mut out, *b"IEND", &[]);
    zbm_sys::wipe_vec(&mut data);
    out
}

/// The PNG file of what `frame` shows, as reference images keep it.
pub fn of_frame(frame: &Frame) -> Vec<u8> {
    frame_at(frame, LEVEL)
}

/// The PNG file of what `frame` shows, as a screenshot: compressed fast. The
/// caller wipes the file once it is saved.
pub fn screenshot(frame: &Frame) -> Vec<u8> {
    frame_at(frame, FAST)
}

/// The PNG file of `frame` at `level`, its pixels wiped once encoded.
fn frame_at(frame: &Frame, level: u8) -> Vec<u8> {
    let (width, height) = frame.size();
    let mut pixels: Vec<Rgb> = Vec::with_capacity(frame_pixels(width, height));
    pixels.extend(
        (0..height)
            .filter_map(|y| frame.row(y))
            .flat_map(|row| row.iter().map(|&pixel| Rgb::from_xrgb(pixel))),
    );
    let file = encode(width, &pixels, level);
    zbm_sys::wipe_values(&mut pixels, Rgb(0, 0, 0));
    file
}

/// How many pixels a frame of `width` by `height` holds.
fn frame_pixels(width: u32, height: u32) -> usize {
    usize::try_from(u64::from(width) * u64::from(height)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::frame::Area;

    #[test]
    fn the_crc_is_the_standard_one() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn an_image_is_a_png_file_whose_data_gives_back_its_pixels() {
        let pixels = [Rgb(255, 0, 0), Rgb(0, 255, 0), Rgb(0, 0, 255), Rgb(1, 2, 3)];
        let file = png(2, &pixels);
        assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR: 13 bytes, 2 × 2, RGB.
        assert_eq!(&file[8..16], b"\0\0\0\x0dIHDR");
        assert_eq!(&file[16..29], b"\0\0\0\x02\0\0\0\x02\x08\x02\0\0\0");
        assert_eq!(&file[file.len() - 12..], b"\0\0\0\0IEND\xae\x42\x60\x82");
        let length = u32::from_be_bytes(file[33..37].try_into().unwrap()) as usize;
        assert_eq!(&file[37..41], b"IDAT");
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&file[41..41 + length]).unwrap();
        assert_eq!(raw, [0, 255, 0, 0, 0, 255, 0, 0, 0, 0, 255, 1, 2, 3]);
        // The same pixels, the same bytes; a part of a row is left out.
        assert_eq!(png(2, &pixels), file);
        assert_eq!(png(3, &pixels)[16..24], *b"\0\0\0\x03\0\0\0\x01");
    }

    #[test]
    fn a_frame_gives_its_pixels() {
        let mut frame = Frame::new(2, 1);
        let area = Area {
            x: 1,
            y: 0,
            width: 1,
            height: 1,
        };
        frame.fill(area, Rgb(9, 8, 7));
        assert_eq!(of_frame(&frame), png(2, &[Rgb::BLACK, Rgb(9, 8, 7)]));
    }

    /// The raw rows in the `IDAT` chunk of `file`.
    fn rows_of(file: &[u8]) -> Vec<u8> {
        let length = u32::from_be_bytes(file[33..37].try_into().unwrap()) as usize;
        miniz_oxide::inflate::decompress_to_vec_zlib(&file[41..41 + length]).unwrap()
    }

    #[test]
    fn a_screenshot_holds_the_frame_s_pixels_compressed_fast_in_one_allocation() {
        let mut frame = Frame::new(64, 32);
        for x in 0..64 {
            let area = Area {
                x,
                y: x % 32,
                width: 1,
                height: 32 - x % 32,
            };
            let shade = u8::try_from(x).unwrap();
            frame.fill(area, Rgb(shade * 4, 255 - shade, 7));
        }
        let shot = screenshot(&frame);
        let kept = of_frame(&frame);

        assert_eq!(rows_of(&shot), rows_of(&kept));
        assert_eq!(shot.len(), shot.capacity());
    }
}
