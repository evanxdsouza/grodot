//! Decode PNG/BMP fixtures written by Pillow (and a hand-written Adam7/BMPv4
//! encoder) and compare with reference RGBA bytes, byte for byte.
//! Regenerate with `python3 tools/gen_image_fixtures.py`.

use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/images")
}

fn check(name: &str, ext: &str, w: u32, h: u32) {
    let bytes = std::fs::read(dir().join(format!("{}.{}", name, ext))).unwrap();
    let expect = std::fs::read(dir().join(format!("{}.rgba", name))).unwrap();
    let img = grodot_core::image::decode(&bytes).unwrap_or_else(|e| panic!("{}: {}", name, e));
    assert_eq!((img.width, img.height), (w, h), "{} size", name);
    if img.rgba != expect {
        let i = img.rgba.iter().zip(expect.iter()).position(|(a, b)| a != b).unwrap();
        let p = i / 4 * 4;
        panic!(
            "{}: first difference at pixel {}: got {:?}, want {:?}",
            name,
            i / 4,
            &img.rgba[p..p + 4],
            &expect[p..p + 4]
        );
    }
}

#[test]
fn png_rgba8_dynamic_huffman() {
    check("rgba8", "png", 37, 23);
}
#[test]
fn png_rgb8_stored_blocks() {
    check("rgb8_level0", "png", 37, 23);
}
#[test]
fn png_rgb8_fast_compression() {
    check("rgb8_level1", "png", 37, 23);
}
#[test]
fn png_gray_and_gray_alpha() {
    check("gray8", "png", 37, 23);
    check("gray_alpha8", "png", 37, 23);
    check("gray1", "png", 37, 23);
    check("gray16", "png", 11, 7);
}
#[test]
fn png_palettes() {
    check("palette4", "png", 37, 23);
    check("palette8", "png", 37, 23);
    check("palette_trns", "png", 16, 9);
}
#[test]
fn png_adam7_interlaced() {
    check("interlaced_rgba8", "png", 29, 17);
}
#[test]
fn png_long_back_references() {
    check("big_stripes", "png", 256, 128);
}
#[test]
fn bmp_24_8_and_32_with_alpha() {
    check("bmp24", "bmp", 37, 23);
    check("bmp8", "bmp", 37, 23);
    check("bmp32", "bmp", 37, 23);
}

#[test]
fn corrupt_png_is_an_error_not_a_panic() {
    let mut bytes = std::fs::read(dir().join("rgba8.png")).unwrap();
    let n = bytes.len();
    bytes[n / 2] ^= 0x55;
    assert!(grodot_core::image::decode(&bytes).is_err());
    assert!(grodot_core::image::decode(&bytes[..40]).is_err());
}
