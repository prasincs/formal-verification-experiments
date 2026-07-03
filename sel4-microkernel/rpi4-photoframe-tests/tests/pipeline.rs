//! End-to-end tests for the secure decode pipeline over the real embedded
//! sample images, plus the rejection paths the security design relies on.

use rpi4_photoframe_tests::bounded_alloc::BoundedBumpAllocator;
use rpi4_photoframe_tests::secure_decode::{secure_decode_into, SecureDecodeError};
use rpi4_photoframe_tests::validate::ImageType;

/// Path to the committed sample images (independent of the test's cwd).
fn photo(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../rpi4-photoframe/photos/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

// 1 MB bounded heap is ample for budget checks on the 320x240 samples;
// BMP/QOI decode allocation-free, so nothing actually lands in this pool.
static HEAP: BoundedBumpAllocator<{ 1024 * 1024 }> = BoundedBumpAllocator::new();

#[test]
fn qoi_through_secure_pipeline() {
    let data = photo("sample_gradient.qoi");
    let mut out = vec![0u32; 320 * 240];
    let res = secure_decode_into(&data, &mut out, &HEAP).expect("qoi decode");
    assert_eq!((res.width, res.height), (320, 240));
    assert!(matches!(res.format, ImageType::Qoi));
    // The generator paints a filled circle near (160,108) in (250,210,40).
    assert_eq!(out[108 * 320 + 160], 0xFFFA_D228);
    // Gradient origin is near-black.
    assert_eq!(out[0] & 0x00FF_FFFF, 0x0000_0000);
}

#[test]
fn bmp_through_secure_pipeline() {
    let data = photo("sample_gradient.bmp");
    let mut out = vec![0u32; 320 * 240];
    let res = secure_decode_into(&data, &mut out, &HEAP).expect("bmp decode");
    assert_eq!((res.width, res.height), (320, 240));
    assert!(matches!(res.format, ImageType::Bmp));
    assert_eq!(out[108 * 320 + 160], 0xFFFA_D228);
}

#[test]
fn bmp_and_qoi_decode_identically() {
    // Both sample files were generated from the same source pixels; the QOI
    // (inline decoder) and BMP (tinybmp) paths must agree pixel-for-pixel.
    let mut a = vec![0u32; 320 * 240];
    let mut b = vec![0u32; 320 * 240];
    secure_decode_into(&photo("sample_gradient.qoi"), &mut a, &HEAP).unwrap();
    secure_decode_into(&photo("sample_gradient.bmp"), &mut b, &HEAP).unwrap();
    assert_eq!(a, b, "QOI and BMP decodes diverged");
}

#[test]
fn rejects_bad_magic() {
    let junk = [0u8; 64];
    let mut out = vec![0u32; 320 * 240];
    let err = secure_decode_into(&junk, &mut out, &HEAP).unwrap_err();
    assert!(matches!(err, SecureDecodeError::Validation(_)), "{err:?}");
}

#[test]
fn rejects_undersized_output_buffer() {
    let data = photo("sample_gradient.qoi");
    let mut tiny = vec![0u32; 16]; // far too small for 320x240
    let err = secure_decode_into(&data, &mut tiny, &HEAP).unwrap_err();
    assert!(matches!(err, SecureDecodeError::OutputTooSmall { .. }), "{err:?}");
}

#[test]
fn rejects_over_budget_heap() {
    // A 1 KB heap cannot satisfy the 320x240 output budget estimate.
    static TINY: BoundedBumpAllocator<1024> = BoundedBumpAllocator::new();
    let data = photo("sample_gradient.qoi");
    let mut out = vec![0u32; 320 * 240];
    let err = secure_decode_into(&data, &mut out, &TINY).unwrap_err();
    assert!(matches!(err, SecureDecodeError::ExceedsBudget { .. }), "{err:?}");
}
