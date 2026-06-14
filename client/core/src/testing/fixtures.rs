use crate::model::Screenshot;

/// Encode a 1×1 transparent RGBA image as PNG bytes using the `image` crate.
/// Generated at call time so the bytes are always structurally correct and pass
/// the CRC checks performed by the PNG decoder inside `ImagePipeline::process`.
pub fn tiny_png_bytes() -> Vec<u8> {
    let img = image::RgbaImage::new(1, 1);
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode tiny test PNG");
    bytes
}

pub fn tiny_png_screenshot() -> Screenshot {
    Screenshot {
        captured_at_ms: 0,
        bytes: tiny_png_bytes(),
        content_type: "image/png".to_string(),
    }
}
