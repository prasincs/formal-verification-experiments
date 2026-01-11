//! # Multi-Format Image Decoder (No Allocation Required)
//!
//! Supports multiple image formats without heap allocation:
//! - **BMP** - Uncompressed bitmap (tinybmp)
//! - **TGA** - Targa with RLE compression (tinytga)
//! - **QOI** - Quite OK Image format (inline implementation)
//!
//! ## Security Note
//!
//! All decoders operate on untrusted input. In the full 3-PD architecture,
//! this module would run in an isolated Decoder PD with no framebuffer access.
//!
//! ## Why QOI?
//!
//! QOI is ideal for embedded systems:
//! - ~30% the size of BMP (lossless)
//! - 3-4x faster to decode than PNG
//! - Simple format (100 lines to implement)
//! - No heap allocation needed

/// Supported image formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Bmp,
    Tga,
    Qoi,
    Unknown,
}

impl ImageFormat {
    /// Detect format from magic bytes
    pub fn detect(data: &[u8]) -> Self {
        if data.len() < 4 {
            return ImageFormat::Unknown;
        }

        // BMP: starts with "BM"
        if data[0] == b'B' && data[1] == b'M' {
            return ImageFormat::Bmp;
        }

        // QOI: starts with "qoif"
        if &data[0..4] == b"qoif" {
            return ImageFormat::Qoi;
        }

        // TGA has no reliable magic bytes
        ImageFormat::Unknown
    }
}

/// Error types for decoding
#[derive(Debug, Clone, Copy)]
pub enum DecodeError {
    InvalidFormat,
    InvalidHeader,
    InvalidDimensions,
    BufferTooSmall,
    UnsupportedFormat,
    CorruptedData,
}

// ============================================================================
// BMP DECODER (using tinybmp)
// ============================================================================

/// Decode a BMP image to ARGB32 pixels
pub fn decode_bmp(data: &[u8], output: &mut [u32]) -> Result<(u32, u32), DecodeError> {
    use tinybmp::Bmp;
    use embedded_graphics_core::pixelcolor::Rgb888;
    use embedded_graphics_core::prelude::*;

    let bmp = Bmp::<Rgb888>::from_slice(data)
        .map_err(|_| DecodeError::InvalidFormat)?;

    let width = bmp.size().width;
    let height = bmp.size().height;

    if output.len() < (width * height) as usize {
        return Err(DecodeError::BufferTooSmall);
    }

    for pixel in bmp.pixels() {
        let x = pixel.0.x as u32;
        let y = pixel.0.y as u32;
        let color = pixel.1;

        if x < width && y < height {
            let idx = (y * width + x) as usize;
            output[idx] = 0xFF000000
                | ((color.r() as u32) << 16)
                | ((color.g() as u32) << 8)
                | (color.b() as u32);
        }
    }

    Ok((width, height))
}

// ============================================================================
// TGA DECODER (using tinytga)
// ============================================================================

/// Decode a TGA image to ARGB32 pixels
pub fn decode_tga(data: &[u8], output: &mut [u32]) -> Result<(u32, u32), DecodeError> {
    use tinytga::Tga;
    use embedded_graphics_core::pixelcolor::Rgb888;
    use embedded_graphics_core::prelude::*;

    let tga = Tga::<Rgb888>::from_slice(data)
        .map_err(|_| DecodeError::InvalidFormat)?;

    let width = tga.size().width;
    let height = tga.size().height;

    if output.len() < (width * height) as usize {
        return Err(DecodeError::BufferTooSmall);
    }

    for pixel in tga.pixels() {
        let x = pixel.0.x as u32;
        let y = pixel.0.y as u32;
        let color = pixel.1;

        if x < width && y < height {
            let idx = (y * width + x) as usize;
            output[idx] = 0xFF000000
                | ((color.r() as u32) << 16)
                | ((color.g() as u32) << 8)
                | (color.b() as u32);
        }
    }

    Ok((width, height))
}

// ============================================================================
// QOI DECODER (inline, no dependencies, no allocation)
// ============================================================================

/// QOI operation codes
mod qoi {
    pub const OP_INDEX: u8 = 0x00;  // 00xxxxxx
    pub const OP_DIFF: u8 = 0x40;   // 01xxxxxx
    pub const OP_LUMA: u8 = 0x80;   // 10xxxxxx
    pub const OP_RUN: u8 = 0xC0;    // 11xxxxxx
    pub const OP_RGB: u8 = 0xFE;    // 11111110
    pub const OP_RGBA: u8 = 0xFF;   // 11111111
    pub const MASK_2: u8 = 0xC0;    // Top 2 bits

    /// QOI color hash for index lookup
    #[inline]
    pub fn hash(r: u8, g: u8, b: u8, a: u8) -> usize {
        ((r as usize).wrapping_mul(3)
            .wrapping_add((g as usize).wrapping_mul(5))
            .wrapping_add((b as usize).wrapping_mul(7))
            .wrapping_add((a as usize).wrapping_mul(11)))
            % 64
    }
}

/// Decode a QOI image to ARGB32 pixels (no allocation required)
///
/// QOI format specification: https://qoiformat.org/qoi-specification.pdf
pub fn decode_qoi(data: &[u8], output: &mut [u32]) -> Result<(u32, u32), DecodeError> {
    // Minimum QOI file: 14 byte header + 8 byte end marker
    if data.len() < 22 {
        return Err(DecodeError::InvalidFormat);
    }

    // Check magic "qoif"
    if &data[0..4] != b"qoif" {
        return Err(DecodeError::InvalidFormat);
    }

    // Parse header (big-endian)
    let width = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let height = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let _channels = data[12];  // 3 = RGB, 4 = RGBA
    let _colorspace = data[13]; // 0 = sRGB, 1 = linear

    // Validate dimensions
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(DecodeError::InvalidDimensions);
    }

    let total_pixels = (width as usize) * (height as usize);
    if output.len() < total_pixels {
        return Err(DecodeError::BufferTooSmall);
    }

    // Decoding state
    let mut index: [[u8; 4]; 64] = [[0, 0, 0, 255]; 64];  // Recently seen colors
    let mut px: [u8; 4] = [0, 0, 0, 255];  // Current pixel (RGBA)

    let mut pos = 14;  // Start after header
    let mut pixel_idx = 0;
    let end = data.len() - 8;  // Stop before 8-byte end marker

    while pixel_idx < total_pixels && pos < end {
        let b1 = data[pos];

        if b1 == qoi::OP_RGB {
            // RGB literal
            if pos + 3 >= data.len() {
                return Err(DecodeError::CorruptedData);
            }
            px[0] = data[pos + 1];
            px[1] = data[pos + 2];
            px[2] = data[pos + 3];
            pos += 4;
        } else if b1 == qoi::OP_RGBA {
            // RGBA literal
            if pos + 4 >= data.len() {
                return Err(DecodeError::CorruptedData);
            }
            px[0] = data[pos + 1];
            px[1] = data[pos + 2];
            px[2] = data[pos + 3];
            px[3] = data[pos + 4];
            pos += 5;
        } else {
            match b1 & qoi::MASK_2 {
                qoi::OP_INDEX => {
                    // Index into recent colors
                    let idx = (b1 & 0x3F) as usize;
                    px = index[idx];
                    pos += 1;
                }
                qoi::OP_DIFF => {
                    // Small RGB difference (-2..1)
                    let dr = ((b1 >> 4) & 0x03).wrapping_sub(2);
                    let dg = ((b1 >> 2) & 0x03).wrapping_sub(2);
                    let db = (b1 & 0x03).wrapping_sub(2);
                    px[0] = px[0].wrapping_add(dr);
                    px[1] = px[1].wrapping_add(dg);
                    px[2] = px[2].wrapping_add(db);
                    pos += 1;
                }
                qoi::OP_LUMA => {
                    // Larger difference with luma
                    if pos + 1 >= data.len() {
                        return Err(DecodeError::CorruptedData);
                    }
                    let b2 = data[pos + 1];
                    let dg = (b1 & 0x3F).wrapping_sub(32);
                    let dr = ((b2 >> 4) & 0x0F).wrapping_sub(8).wrapping_add(dg);
                    let db = (b2 & 0x0F).wrapping_sub(8).wrapping_add(dg);
                    px[0] = px[0].wrapping_add(dr);
                    px[1] = px[1].wrapping_add(dg);
                    px[2] = px[2].wrapping_add(db);
                    pos += 2;
                }
                qoi::OP_RUN => {
                    // Run of same pixel (1-62 pixels)
                    let run = ((b1 & 0x3F) + 1) as usize;
                    let color = 0xFF000000
                        | ((px[0] as u32) << 16)
                        | ((px[1] as u32) << 8)
                        | (px[2] as u32);

                    for _ in 0..run.min(total_pixels - pixel_idx) {
                        output[pixel_idx] = color;
                        pixel_idx += 1;
                    }
                    pos += 1;

                    // Update index and continue (don't emit pixel again)
                    let hash = qoi::hash(px[0], px[1], px[2], px[3]);
                    index[hash] = px;
                    continue;
                }
                _ => {
                    return Err(DecodeError::CorruptedData);
                }
            }
        }

        // Store in index
        let hash = qoi::hash(px[0], px[1], px[2], px[3]);
        index[hash] = px;

        // Write pixel
        output[pixel_idx] = 0xFF000000
            | ((px[0] as u32) << 16)
            | ((px[1] as u32) << 8)
            | (px[2] as u32);
        pixel_idx += 1;
    }

    // Fill any remaining pixels (shouldn't happen with valid files)
    while pixel_idx < total_pixels {
        output[pixel_idx] = 0xFF000000;
        pixel_idx += 1;
    }

    Ok((width, height))
}

// ============================================================================
// AUTO-DETECT DECODER
// ============================================================================

/// Decode any supported image format (auto-detect)
pub fn decode_auto(data: &[u8], output: &mut [u32]) -> Result<(u32, u32, ImageFormat), DecodeError> {
    let format = ImageFormat::detect(data);

    match format {
        ImageFormat::Bmp => {
            let (w, h) = decode_bmp(data, output)?;
            Ok((w, h, ImageFormat::Bmp))
        }
        ImageFormat::Qoi => {
            let (w, h) = decode_qoi(data, output)?;
            Ok((w, h, ImageFormat::Qoi))
        }
        ImageFormat::Tga | ImageFormat::Unknown => {
            // Try TGA (no magic bytes)
            if let Ok((w, h)) = decode_tga(data, output) {
                return Ok((w, h, ImageFormat::Tga));
            }
            // Try BMP as fallback
            if let Ok((w, h)) = decode_bmp(data, output) {
                return Ok((w, h, ImageFormat::Bmp));
            }
            Err(DecodeError::UnsupportedFormat)
        }
    }
}

/// Decode with explicit format specification
pub fn decode_format(
    data: &[u8],
    format: ImageFormat,
    output: &mut [u32],
) -> Result<(u32, u32), DecodeError> {
    match format {
        ImageFormat::Bmp => decode_bmp(data, output),
        ImageFormat::Tga => decode_tga(data, output),
        ImageFormat::Qoi => decode_qoi(data, output),
        ImageFormat::Unknown => Err(DecodeError::UnsupportedFormat),
    }
}

// ============================================================================
// FORMAT COMPARISON
// ============================================================================

/// Comparison of supported formats for the photo frame:
///
/// | Format | Compression | Decode Speed | File Size | Alloc? | Best For |
/// |--------|-------------|--------------|-----------|--------|----------|
/// | BMP    | None        | Very Fast    | Largest   | No     | Simple images |
/// | TGA    | RLE         | Fast         | Large     | No     | Graphics with runs |
/// | QOI    | LZ-like     | Fast         | ~30% BMP  | No     | Photos, general use |
/// | PNG*   | Deflate     | Slow         | Small     | Yes    | (not supported) |
/// | JPEG*  | DCT         | Medium       | Smallest  | Yes    | (not supported) |
///
/// *Requires heap allocation, not included
///
/// **Recommendation:** Use QOI for best balance of size and speed.
/// Convert existing images with: `qoiconv input.png output.qoi`
#[allow(dead_code)]
mod format_info {}
