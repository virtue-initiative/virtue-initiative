//! Runs a screenshot through the same OCR redaction and image pipeline a capture goes through,
//! for showing what an accountability partner sees (e.g. the landing page's before/after).
//!
//! ```sh
//! cargo run --release -p virtue-core --example redaction_demo -- SHOT.png OUT_DIR
//! ```
//!
//! Writes `OUT_DIR/redacted.png` (text blacked out, full size) and `OUT_DIR/shared.webp` (the
//! shrunk, blurred image that gets uploaded).

use std::path::PathBuf;

use virtue_core::module::screenshot::image_pipeline::ImagePipeline;
use virtue_core::module::screenshot::{load_ocr, redact_text_regions};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(out)) = (args.next(), args.next()) else {
        eprintln!("usage: redaction_demo SHOT.png OUT_DIR");
        std::process::exit(2);
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("create output dir");

    let bytes = std::fs::read(&input).expect("read screenshot");
    let decoded = image::load_from_memory(&bytes).expect("decode screenshot");
    let ocr = load_ocr().expect("OCR engine unavailable on this platform");

    // Calls `detect` directly rather than `redact_if_ocr`, which would quietly hand back the
    // unredacted frame if OCR failed.
    let detected = ocr.detect(&bytes).expect("OCR failed");
    if detected.regions.is_empty() {
        eprintln!("OCR found no text, so redacted.png will match the input");
    } else {
        eprintln!("blacked out {} text regions", detected.regions.len());
    }
    let redacted = redact_text_regions(decoded, &detected.regions);
    redacted
        .save(out.join("redacted.png"))
        .expect("write redacted.png");

    let shared = ImagePipeline
        .process_image(redacted, 0)
        .expect("image pipeline");
    std::fs::write(out.join("shared.webp"), shared.bytes).expect("write shared.webp");
}
