//! PNG files of drawn frames, for the snapshot tests that compare what a
//! console or the menu draws with reference images in the repository.
//!
//! The images are RGB, 8 bits a channel, without filters, compressed with
//! `miniz_oxide` at a fixed level: the same pixels always give the same
//! bytes, so a snapshot compares pixels by comparing files, and any image
//! viewer shows a changed one.

/// The compression level: the best, as the images are kept.
const LEVEL: u8 = 9;

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
    let length = u32::try_from(data.len()).expect("a chunk fits in 4 GiB");
    out.extend_from_slice(&length.to_be_bytes());
    let start = out.len();
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// The PNG file of an image `width` pixels wide whose rows are `rows`,
/// each pixel as (red, green, blue).
pub fn png(width: u32, rows: &[Vec<(u8, u8, u8)>]) -> Vec<u8> {
    let height = u32::try_from(rows.len()).expect("fewer than 4 billion rows");
    let mut raw = Vec::with_capacity(rows.len() * (1 + 3 * width as usize));
    for row in rows {
        assert_eq!(row.len(), width as usize, "every row is the image's width");
        // Filter type 0: the bytes as they are.
        raw.push(0);
        for &(r, g, b) in row {
            raw.extend_from_slice(&[r, g, b]);
        }
    }
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // 8 bits a channel, colour type 2 (RGB), deflate, no filter choice, no
    // interlace.
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, *b"IHDR", &header);
    let data = miniz_oxide::deflate::compress_to_vec_zlib(&raw, LEVEL);
    chunk(&mut out, *b"IDAT", &data);
    chunk(&mut out, *b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_crc_is_the_standard_one() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn an_image_is_a_png_file_whose_data_gives_back_its_pixels() {
        let rows = vec![vec![(255, 0, 0), (0, 255, 0)], vec![(0, 0, 255), (1, 2, 3)]];
        let file = png(2, &rows);

        assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR: 13 bytes, 2 × 2, RGB.
        assert_eq!(&file[8..16], b"\0\0\0\x0dIHDR");
        assert_eq!(&file[16..29], b"\0\0\0\x02\0\0\0\x02\x08\x02\0\0\0");
        assert_eq!(&file[file.len() - 12..], b"\0\0\0\0IEND\xae\x42\x60\x82");
        let length = u32::from_be_bytes(file[33..37].try_into().unwrap()) as usize;
        assert_eq!(&file[37..41], b"IDAT");
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&file[41..41 + length]).unwrap();
        assert_eq!(raw, [0, 255, 0, 0, 0, 255, 0, 0, 0, 0, 255, 1, 2, 3]);
        // The same pixels, the same bytes.
        assert_eq!(png(2, &rows), file);
    }

    #[test]
    #[should_panic(expected = "every row is the image's width")]
    fn rows_of_another_width_are_refused() {
        png(3, &[vec![(0, 0, 0)]]);
    }
}
