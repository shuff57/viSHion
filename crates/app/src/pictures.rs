//! Page pictures for the review screen's tools: prepare a picked file (js/image.js `prepare`: longest side at
//! most 1600 px, JPEG quality 0.9) and turn a stored page a quarter turn. Pure Rust (`image` crate), the same
//! on native and wasm, so there is no platform canvas involved. Straightening needs the imaging port
//! (phase 5) and is not here.
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::DynamicImage;

pub const MAX_SIDE: u32 = 1600;
pub const QUALITY: u8 = 90;
/// js/image.js `MAX_BYTES`.
pub const MAX_BYTES: usize = 40 * 1024 * 1024;

fn encode(im: &DynamicImage) -> Result<Vec<u8>, String> {
    let rgb = im.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, QUALITY).encode_image(&rgb).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Decode a picked file and re-encode it as the JPEG the assignment stores.
pub fn prepare(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > MAX_BYTES {
        return Err("The file is larger than 40 MB.".into());
    }
    let im = image::load_from_memory(bytes).map_err(|_| "That is not a picture this app can read.".to_string())?;
    let im = if im.width().max(im.height()) > MAX_SIDE { im.resize(MAX_SIDE, MAX_SIDE, FilterType::Triangle) } else { im };
    encode(&im)
}

/// A quarter turn clockwise (js `OCR.image.rotate(blob, 90)`).
pub fn rotate90(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let im = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    encode(&im.rotate90())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let im = image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30]));
        let mut out = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(im).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn size(b: &[u8]) -> (u32, u32) {
        let i = image::load_from_memory(b).unwrap();
        (i.width(), i.height())
    }

    #[test]
    fn prepare_shrinks_to_1600_and_stores_a_jpeg() {
        let out = prepare(&png(3200, 1200)).unwrap();
        assert_eq!(&out[..2], &[0xFF, 0xD8], "JPEG magic");
        assert_eq!(size(&out), (1600, 600));
        assert_eq!(size(&prepare(&png(300, 400)).unwrap()), (300, 400), "small pictures are not enlarged");
    }

    #[test]
    fn rotate_swaps_the_sides() {
        let j = prepare(&png(300, 200)).unwrap();
        assert_eq!(size(&rotate90(&j).unwrap()), (200, 300));
    }

    #[test]
    fn junk_is_refused_with_a_sentence() {
        assert!(prepare(b"not a picture").unwrap_err().contains("picture"));
    }
}
