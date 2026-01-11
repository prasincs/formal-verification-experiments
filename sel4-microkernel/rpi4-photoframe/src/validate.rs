//! # Image Header Validation
//!
//! Pre-validates image headers before full decode to catch:
//! - Invalid magic bytes
//! - Unreasonable dimensions
//! - Truncated files
//!
//! This runs BEFORE any allocation happens, rejecting obviously
//! malformed files at minimal cost.

/// Maximum image dimensions we'll accept
pub const MAX_WIDTH: u32 = 4096;
pub const MAX_HEIGHT: u32 = 4096;
pub const MAX_PIXELS: u64 = 16 * 1024 * 1024; // 16 megapixels

/// Validation errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    /// File too small to contain header
    TooSmall,
    /// Invalid magic bytes for format
    InvalidMagic,
    /// Header is truncated or malformed
    InvalidHeader,
    /// Width or height is zero
    ZeroDimension,
    /// Dimensions exceed maximum allowed
    TooLarge,
    /// Total pixels exceed limit
    TooManyPixels,
    /// Unsupported format variant
    UnsupportedVariant,
}

/// Validated image information
#[derive(Debug, Clone, Copy)]
pub struct ValidatedImage {
    pub width: u32,
    pub height: u32,
    pub format: ImageType,
    /// Estimated decode memory (conservative)
    pub estimated_memory: usize,
}

/// Detected image type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    Jpeg,
    Png,
    Bmp,
    Tga,
    Qoi,
    Unknown,
}

impl ValidatedImage {
    /// Calculate output buffer size needed
    pub fn output_size(&self) -> usize {
        (self.width as usize) * (self.height as usize) * 4
    }
}

// ============================================================================
// JPEG VALIDATION
// ============================================================================

/// Validate JPEG header and extract dimensions.
///
/// Parses just enough to find SOF marker (Start of Frame) which
/// contains the image dimensions. Does not decode the image.
pub fn validate_jpeg(data: &[u8]) -> Result<ValidatedImage, ValidationError> {
    // Minimum JPEG: SOI + SOF + EOI
    if data.len() < 20 {
        return Err(ValidationError::TooSmall);
    }

    // Check SOI (Start of Image) marker
    if data[0] != 0xFF || data[1] != 0xD8 {
        return Err(ValidationError::InvalidMagic);
    }

    // Scan for SOF0 (baseline) or SOF2 (progressive) marker
    let mut pos = 2;
    while pos + 4 < data.len() {
        // All markers start with 0xFF
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }

        // Skip padding bytes (0xFF 0xFF...)
        while pos < data.len() && data[pos] == 0xFF {
            pos += 1;
        }

        if pos >= data.len() {
            break;
        }

        let marker = data[pos];
        pos += 1;

        // Markers without length field
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            continue;
        }

        // Get segment length
        if pos + 2 > data.len() {
            return Err(ValidationError::InvalidHeader);
        }
        let length = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        if length < 2 {
            return Err(ValidationError::InvalidHeader);
        }

        // SOF0 (baseline DCT) or SOF2 (progressive DCT)
        if marker == 0xC0 || marker == 0xC2 {
            // SOF segment: length(2) + precision(1) + height(2) + width(2) + ...
            if pos + 7 > data.len() {
                return Err(ValidationError::InvalidHeader);
            }

            let height = u16::from_be_bytes([data[pos + 3], data[pos + 4]]) as u32;
            let width = u16::from_be_bytes([data[pos + 5], data[pos + 6]]) as u32;

            // Validate dimensions
            if width == 0 || height == 0 {
                return Err(ValidationError::ZeroDimension);
            }
            if width > MAX_WIDTH || height > MAX_HEIGHT {
                return Err(ValidationError::TooLarge);
            }
            if (width as u64) * (height as u64) > MAX_PIXELS {
                return Err(ValidationError::TooManyPixels);
            }

            // Estimate decode memory: output + huffman tables + DCT buffers
            let output_size = (width as usize) * (height as usize) * 4;
            let decode_overhead = 2 * 1024 * 1024; // ~2MB for decode state
            let estimated_memory = output_size + decode_overhead;

            return Ok(ValidatedImage {
                width,
                height,
                format: ImageType::Jpeg,
                estimated_memory,
            });
        }

        // SOF markers we don't support
        if (0xC1..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
            return Err(ValidationError::UnsupportedVariant);
        }

        // Skip to next marker
        pos += length;
    }

    Err(ValidationError::InvalidHeader)
}

// ============================================================================
// PNG VALIDATION
// ============================================================================

/// PNG signature bytes
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Validate PNG header and extract dimensions.
///
/// PNG files must start with the signature followed by an IHDR chunk.
pub fn validate_png(data: &[u8]) -> Result<ValidatedImage, ValidationError> {
    // Minimum PNG: signature(8) + IHDR chunk header(8) + IHDR data(13) + CRC(4)
    if data.len() < 33 {
        return Err(ValidationError::TooSmall);
    }

    // Check PNG signature
    if &data[0..8] != PNG_SIGNATURE {
        return Err(ValidationError::InvalidMagic);
    }

    // Parse IHDR chunk (must be first)
    let ihdr_length = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if ihdr_length != 13 {
        return Err(ValidationError::InvalidHeader);
    }

    // Check IHDR type
    if &data[12..16] != b"IHDR" {
        return Err(ValidationError::InvalidHeader);
    }

    // Parse IHDR data
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    let bit_depth = data[24];
    let color_type = data[25];
    let compression = data[26];
    let filter = data[27];
    let interlace = data[28];

    // Validate dimensions
    if width == 0 || height == 0 {
        return Err(ValidationError::ZeroDimension);
    }
    if width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(ValidationError::TooLarge);
    }
    if (width as u64) * (height as u64) > MAX_PIXELS {
        return Err(ValidationError::TooManyPixels);
    }

    // Basic sanity checks
    if compression != 0 || filter != 0 {
        return Err(ValidationError::UnsupportedVariant);
    }
    if !matches!(bit_depth, 1 | 2 | 4 | 8 | 16) {
        return Err(ValidationError::UnsupportedVariant);
    }
    if !matches!(color_type, 0 | 2 | 3 | 4 | 6) {
        return Err(ValidationError::UnsupportedVariant);
    }

    // Estimate memory: output + zlib inflate + filter buffer
    let output_size = (width as usize) * (height as usize) * 4;
    let decode_overhead = if interlace != 0 {
        output_size // Interlaced needs extra buffer
    } else {
        1024 * 1024 // ~1MB for zlib state
    };
    let estimated_memory = output_size + decode_overhead;

    Ok(ValidatedImage {
        width,
        height,
        format: ImageType::Png,
        estimated_memory,
    })
}

// ============================================================================
// BMP VALIDATION (simpler format)
// ============================================================================

/// Validate BMP header.
pub fn validate_bmp(data: &[u8]) -> Result<ValidatedImage, ValidationError> {
    if data.len() < 26 {
        return Err(ValidationError::TooSmall);
    }

    // BMP signature
    if data[0] != b'B' || data[1] != b'M' {
        return Err(ValidationError::InvalidMagic);
    }

    // DIB header starts at offset 14
    // BITMAPINFOHEADER: width at offset 18, height at offset 22
    let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let height = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);

    // Height can be negative (top-down DIB)
    let width = width.unsigned_abs();
    let height = height.unsigned_abs();

    if width == 0 || height == 0 {
        return Err(ValidationError::ZeroDimension);
    }
    if width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(ValidationError::TooLarge);
    }
    if (width as u64) * (height as u64) > MAX_PIXELS {
        return Err(ValidationError::TooManyPixels);
    }

    let output_size = (width as usize) * (height as usize) * 4;

    Ok(ValidatedImage {
        width,
        height,
        format: ImageType::Bmp,
        estimated_memory: output_size, // BMP has minimal decode overhead
    })
}

// ============================================================================
// QOI VALIDATION
// ============================================================================

/// Validate QOI header.
pub fn validate_qoi(data: &[u8]) -> Result<ValidatedImage, ValidationError> {
    if data.len() < 14 {
        return Err(ValidationError::TooSmall);
    }

    // QOI magic
    if &data[0..4] != b"qoif" {
        return Err(ValidationError::InvalidMagic);
    }

    let width = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let height = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);

    if width == 0 || height == 0 {
        return Err(ValidationError::ZeroDimension);
    }
    if width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(ValidationError::TooLarge);
    }
    if (width as u64) * (height as u64) > MAX_PIXELS {
        return Err(ValidationError::TooManyPixels);
    }

    let output_size = (width as usize) * (height as usize) * 4;

    Ok(ValidatedImage {
        width,
        height,
        format: ImageType::Qoi,
        estimated_memory: output_size + 256, // QOI has minimal state (64-entry table)
    })
}

// ============================================================================
// AUTO-DETECT AND VALIDATE
// ============================================================================

/// Detect format and validate header.
pub fn validate_auto(data: &[u8]) -> Result<ValidatedImage, ValidationError> {
    if data.len() < 4 {
        return Err(ValidationError::TooSmall);
    }

    // Try to detect by magic bytes
    if data[0] == 0xFF && data[1] == 0xD8 {
        return validate_jpeg(data);
    }
    if data.len() >= 8 && &data[0..8] == PNG_SIGNATURE {
        return validate_png(data);
    }
    if data[0] == b'B' && data[1] == b'M' {
        return validate_bmp(data);
    }
    if &data[0..4] == b"qoif" {
        return validate_qoi(data);
    }

    Err(ValidationError::InvalidMagic)
}

/// Check if estimated memory fits in allocator budget.
pub fn fits_in_budget(validated: &ValidatedImage, budget: usize) -> bool {
    validated.estimated_memory <= budget
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jpeg_validation() {
        // Minimal valid JPEG-like header (simplified)
        let data = [
            0xFF, 0xD8,             // SOI
            0xFF, 0xC0,             // SOF0
            0x00, 0x0B,             // Length: 11
            0x08,                   // Precision
            0x00, 0x64,             // Height: 100
            0x00, 0xC8,             // Width: 200
            0x03,                   // Components
            0x01, 0x11, 0x00,       // Component data
        ];

        let result = validate_jpeg(&data);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.width, 200);
        assert_eq!(info.height, 100);
    }

    #[test]
    fn test_png_validation() {
        let mut data = Vec::new();
        data.extend_from_slice(PNG_SIGNATURE);
        // IHDR chunk
        data.extend_from_slice(&[0, 0, 0, 13]); // Length
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&[0, 0, 0, 100]); // Width: 100
        data.extend_from_slice(&[0, 0, 0, 50]);  // Height: 50
        data.push(8);   // Bit depth
        data.push(2);   // Color type (RGB)
        data.push(0);   // Compression
        data.push(0);   // Filter
        data.push(0);   // Interlace
        data.extend_from_slice(&[0, 0, 0, 0]); // CRC

        let result = validate_png(&data);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.width, 100);
        assert_eq!(info.height, 50);
    }

    #[test]
    fn test_oversized_rejection() {
        // JPEG with dimensions exceeding MAX
        let data = [
            0xFF, 0xD8,
            0xFF, 0xC0,
            0x00, 0x0B,
            0x08,
            0xFF, 0xFF,  // Height: 65535 (too large)
            0xFF, 0xFF,  // Width: 65535 (too large)
            0x03,
            0x01, 0x11, 0x00,
        ];

        let result = validate_jpeg(&data);
        assert_eq!(result.err(), Some(ValidationError::TooLarge));
    }
}
