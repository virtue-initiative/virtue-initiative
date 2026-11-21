//! Combines one `screencapture` image per display into a single screenshot,
//! placed as the displays are arranged (CLIENT-018).

use std::io::Cursor;

use anyhow::{Context, Result, anyhow};
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::{self, FilterType};
use image::{ImageEncoder, Rgba, RgbaImage};

/// A display's rectangle in macOS's global display space, in points. The main
/// display's top-left is the origin, so a display left of or above it has a
/// negative `x` or `y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where one captured image goes in the combined screenshot, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    pub placements: Vec<Placement>,
}

/// How far an image's horizontal and vertical pixels-per-point may differ
/// and still count as the display it was paired with.
const SCALE_TOLERANCE: f64 = 0.02;

/// Places each captured image (`sizes`, in pixels) on one canvas. When every
/// image pairs up with its display in `displays`, the images keep the
/// displays' arrangement, scaled to the densest display's pixels-per-point
/// so text on a Retina display stays sharp for OCR. Otherwise, as when a
/// display was plugged in or out between listing the displays and
/// capturing, the images go side by side at their own size.
pub fn layout(sizes: &[(u32, u32)], displays: &[DisplayBounds]) -> Layout {
    arranged_layout(sizes, displays).unwrap_or_else(|| side_by_side_layout(sizes))
}

fn arranged_layout(sizes: &[(u32, u32)], displays: &[DisplayBounds]) -> Option<Layout> {
    if sizes.is_empty() || sizes.len() != displays.len() {
        return None;
    }
    let mut scale: f64 = 0.0;
    for (&(w, h), d) in sizes.iter().zip(displays) {
        if d.width <= 0.0 || d.height <= 0.0 || w == 0 || h == 0 {
            return None;
        }
        let sx = f64::from(w) / d.width;
        let sy = f64::from(h) / d.height;
        if (sx - sy).abs() > SCALE_TOLERANCE * sx.max(sy) {
            return None;
        }
        scale = scale.max(sx);
    }
    let min_x = displays.iter().map(|d| d.x).fold(f64::INFINITY, f64::min);
    let min_y = displays.iter().map(|d| d.y).fold(f64::INFINITY, f64::min);
    let px = |points: f64| (points * scale).round() as u32;
    let placements: Vec<Placement> = displays
        .iter()
        .map(|d| Placement {
            x: px(d.x - min_x),
            y: px(d.y - min_y),
            width: px(d.width).max(1),
            height: px(d.height).max(1),
        })
        .collect();
    Some(Layout {
        width: placements.iter().map(|p| p.x + p.width).max()?,
        height: placements.iter().map(|p| p.y + p.height).max()?,
        placements,
    })
}

fn side_by_side_layout(sizes: &[(u32, u32)]) -> Layout {
    let mut x = 0;
    let placements = sizes
        .iter()
        .map(|&(width, height)| {
            let placement = Placement {
                x,
                y: 0,
                width,
                height,
            };
            x += width;
            placement
        })
        .collect();
    Layout {
        width: x,
        height: sizes.iter().map(|&(_, h)| h).max().unwrap_or(0),
        placements,
    }
}

/// Decodes each PNG, places it per [`layout`] on a black canvas, and returns
/// the result as a PNG. One image comes back unchanged.
pub fn combine_pngs(pngs: Vec<Vec<u8>>, displays: &[DisplayBounds]) -> Result<Vec<u8>> {
    if pngs.len() <= 1 {
        return pngs
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no display images to combine"));
    }
    let images = pngs
        .iter()
        .enumerate()
        .map(|(i, png)| {
            image::load_from_memory(png)
                .map(|img| img.into_rgba8())
                .with_context(|| format!("decoding display image {}", i + 1))
        })
        .collect::<Result<Vec<_>>>()?;
    let sizes: Vec<(u32, u32)> = images.iter().map(RgbaImage::dimensions).collect();
    let layout = layout(&sizes, displays);

    let mut canvas = RgbaImage::from_pixel(layout.width, layout.height, Rgba([0, 0, 0, 255]));
    for (img, p) in images.iter().zip(&layout.placements) {
        let x = i64::from(p.x);
        let y = i64::from(p.y);
        if img.dimensions() == (p.width, p.height) {
            imageops::overlay(&mut canvas, img, x, y);
        } else {
            let resized = imageops::resize(img, p.width, p.height, FilterType::Triangle);
            imageops::overlay(&mut canvas, &resized, x, y);
        }
    }

    // The core decodes this straight back, so favor speed over size.
    let mut out = Vec::new();
    PngEncoder::new_with_quality(
        Cursor::new(&mut out),
        CompressionType::Fast,
        PngFilter::Adaptive,
    )
    .write_image(
        canvas.as_raw(),
        layout.width,
        layout.height,
        image::ExtendedColorType::Rgba8,
    )
    .context("encoding combined screenshot")?;
    Ok(out)
}

#[cfg(test)]
// CLIENT-018: macOS combines every display into one image, placed as the
// displays are arranged, or side by side when the captures don't match the
// display list.
mod tests {
    use super::*;

    fn bounds(x: f64, y: f64, width: f64, height: f64) -> DisplayBounds {
        DisplayBounds {
            x,
            y,
            width,
            height,
        }
    }

    fn placement(x: u32, y: u32, width: u32, height: u32) -> Placement {
        Placement {
            x,
            y,
            width,
            height,
        }
    }

    fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let img = RgbaImage::from_pixel(width, height, Rgba(color));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    #[test]
    fn displays_keep_their_arrangement_at_the_densest_scale() {
        // A Retina main display with a 1x external display to its right,
        // lower edge aligned.
        let displays = [
            bounds(0.0, 0.0, 1440.0, 900.0),
            bounds(1440.0, -180.0, 1920.0, 1080.0),
        ];
        let got = layout(&[(2880, 1800), (1920, 1080)], &displays);
        assert_eq!(
            got,
            Layout {
                width: 6720,
                height: 2160,
                placements: vec![
                    placement(0, 360, 2880, 1800),
                    placement(2880, 0, 3840, 2160)
                ],
            }
        );
    }

    #[test]
    fn a_display_left_of_the_main_one_starts_the_canvas() {
        let displays = [
            bounds(0.0, 0.0, 100.0, 50.0),
            bounds(-80.0, 10.0, 80.0, 40.0),
        ];
        let got = layout(&[(100, 50), (80, 40)], &displays);
        assert_eq!(
            got,
            Layout {
                width: 180,
                height: 50,
                placements: vec![placement(80, 0, 100, 50), placement(0, 10, 80, 40)],
            }
        );
    }

    #[test]
    fn captures_that_dont_match_the_displays_go_side_by_side() {
        let side_by_side = Layout {
            width: 180,
            height: 50,
            placements: vec![placement(0, 0, 100, 50), placement(100, 0, 80, 40)],
        };
        let cases: [(&str, Vec<DisplayBounds>); 3] = [
            // A display was unplugged between listing and capturing.
            ("fewer displays", vec![bounds(0.0, 0.0, 100.0, 50.0)]),
            // The second image's shape doesn't fit its display.
            (
                "wrong shape",
                vec![
                    bounds(0.0, 0.0, 100.0, 50.0),
                    bounds(100.0, 0.0, 40.0, 40.0),
                ],
            ),
            (
                "empty bounds",
                vec![bounds(0.0, 0.0, 100.0, 50.0), bounds(100.0, 0.0, 0.0, 0.0)],
            ),
        ];
        for (name, displays) in cases {
            assert_eq!(
                layout(&[(100, 50), (80, 40)], &displays),
                side_by_side,
                "{name}"
            );
        }
    }

    #[test]
    fn one_display_image_is_returned_unchanged() {
        let only = png(4, 2, RED);
        let got = combine_pngs(vec![only.clone()], &[bounds(0.0, 0.0, 4.0, 2.0)]).unwrap();
        assert_eq!(got, only);
    }

    #[test]
    fn two_display_images_combine_into_one_png() {
        // A 2x display at the origin and a 1x display below it, offset right.
        let displays = [bounds(0.0, 0.0, 4.0, 2.0), bounds(2.0, 2.0, 2.0, 1.0)];
        let combined = combine_pngs(vec![png(8, 4, RED), png(2, 1, BLUE)], &displays).unwrap();
        let img = image::load_from_memory(&combined).unwrap().into_rgba8();
        assert_eq!(img.dimensions(), (8, 6));
        assert_eq!(img.get_pixel(0, 0).0, RED);
        assert_eq!(img.get_pixel(7, 3).0, RED);
        // The 1x display is scaled up to 4x2 pixels at (4, 4).
        assert_eq!(img.get_pixel(4, 4).0, BLUE);
        assert_eq!(img.get_pixel(7, 5).0, BLUE);
        // Space no display covers is black.
        assert_eq!(img.get_pixel(0, 5).0, BLACK);
    }

    #[test]
    fn an_image_that_is_not_a_png_fails_the_capture() {
        let err = combine_pngs(vec![png(2, 2, RED), b"not a png".to_vec()], &[]).unwrap_err();
        assert!(format!("{err:#}").contains("display image 2"), "{err:#}");
    }
}
