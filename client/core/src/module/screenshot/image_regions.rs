//! Finds the photo-like regions (embedded images, video frames, thumbnails) inside a
//! screenshot, so the NSFW model can look at each one on its own instead of at the whole
//! screen squashed down to 224×224.
//!
//! The approach is the block classifier used for screen-content coding: UI chrome, text and
//! icons are drawn from a handful of colors, while natural images are not, so a 16×16 block
//! with many distinct colors is almost always part of a photo. The steps are:
//!
//! 1. Count the distinct colors in every 16×16 block and mark the colorful ones.
//! 2. Close one-block holes (flat patches of sky or wall inside a photo) and group the
//!    marked blocks into 4-connected components.
//! 3. Refine each component's block-aligned box at pixel level with an XY-cut: trim
//!    uniform rows and columns from the edges, and split on uniform gutters running all
//!    the way across, which separates images laid out in a grid.
//! 4. Drop pieces that are too small or not mostly colorful, and keep the largest few.
//!
//! The whole pass touches each pixel about once, so it costs a few milliseconds on a
//! 1080p frame.

use image::{DynamicImage, GenericImageView};

/// Side of the square blocks the screen is classified in, in pixels.
const BLOCK: u32 = 16;
/// A block with at least this many distinct colors (out of 256 pixels) counts as photo-like.
/// Anti-aliased text sits well below this; JPEG/scaled photos sit well above it.
const PHOTO_COLORS: u32 = 48;
/// Distinct-color counting stops here; anything above is "plenty".
const COLOR_CAP: u32 = 128;
/// Grayscale and muted photos can fall short of [`PHOTO_COLORS`], so a block also counts
/// as photo-like if it has at least this many colors and no one color covers more than
/// [`MAX_DOMINANT_SHARE`] of it. Text and UI blocks are mostly one background color.
const MUTED_PHOTO_COLORS: u32 = 16;
const MAX_DOMINANT_SHARE: f32 = 0.15;
/// Two pixels count as the same color in a gutter if no channel differs by more than this.
const GUTTER_TOLERANCE: i32 = 10;
/// Pieces narrower or shorter than this are dropped: upscaled to 224×224 they give the
/// model too little to go on, and they're mostly avatars and icons.
pub const MIN_SIDE: u32 = 64;
/// A piece must be at least this share photo-like blocks, measured before hole-closing.
const MIN_PHOTO_FILL: f32 = 0.4;
/// XY-cut recursion limit; a grid of thumbnails needs one level per row/column split.
const MAX_CUT_DEPTH: u32 = 8;

/// An axis-aligned rectangle in screenshot pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    fn right(&self) -> u32 {
        self.x + self.width
    }

    fn bottom(&self) -> u32 {
        self.y + self.height
    }
}

/// Returns up to `max` photo-like regions of `img`, largest first.
pub fn extract_regions(img: &DynamicImage, max: usize) -> Vec<Region> {
    // Screenshots decode to RGBA8 or RGB8; read those in place rather than paying for a
    // full-frame conversion. Anything else is rare enough to just convert.
    let converted;
    let px = match img {
        DynamicImage::ImageRgba8(buf) => Pixels::new(buf.as_raw(), buf.width(), buf.height(), 4),
        DynamicImage::ImageRgb8(buf) => Pixels::new(buf.as_raw(), buf.width(), buf.height(), 3),
        other => {
            converted = other.to_rgb8();
            Pixels::new(converted.as_raw(), converted.width(), converted.height(), 3)
        }
    };
    let (width, height) = img.dimensions();
    if width < MIN_SIDE || height < MIN_SIDE {
        return Vec::new();
    }

    let grid = BlockGrid::classify(&px);
    let closed = grid.closed();

    let mut pieces = Vec::new();
    for component in closed.components() {
        refine(&px, component, 0, &mut pieces);
    }
    pieces.retain(|r| {
        r.width >= MIN_SIDE && r.height >= MIN_SIDE && grid.photo_fill(r) >= MIN_PHOTO_FILL
    });
    pieces.sort_by_key(|r| std::cmp::Reverse(r.area()));
    pieces.truncate(max);
    pieces
}

/// Borrowed view of an 8-bit RGB or RGBA buffer.
struct Pixels<'a> {
    data: &'a [u8],
    width: u32,
    height: u32,
    channels: usize,
}

impl<'a> Pixels<'a> {
    fn new(data: &'a [u8], width: u32, height: u32, channels: usize) -> Self {
        Self {
            data,
            width,
            height,
            channels,
        }
    }

    #[inline]
    fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * self.width as usize + x as usize) * self.channels;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    /// True if every pixel in row `y` between `x0..x1` matches the first one.
    fn row_uniform(&self, y: u32, x0: u32, x1: u32) -> bool {
        let first = self.rgb(x0, y);
        (x0 + 1..x1).all(|x| close(first, self.rgb(x, y)))
    }

    /// True if every pixel in column `x` between `y0..y1` matches the first one.
    fn col_uniform(&self, x: u32, y0: u32, y1: u32) -> bool {
        let first = self.rgb(x, y0);
        (y0 + 1..y1).all(|y| close(first, self.rgb(x, y)))
    }
}

#[inline]
fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).all(|c| (a[c] as i32 - b[c] as i32).abs() <= GUTTER_TOLERANCE)
}

/// One flag per block: is it photo-like.
struct BlockGrid {
    cols: u32,
    rows: u32,
    photo: Vec<bool>,
}

impl BlockGrid {
    fn classify(px: &Pixels) -> Self {
        let cols = px.width.div_ceil(BLOCK);
        let rows = px.height.div_ceil(BLOCK);
        let mut set = ColorSet::new();
        let mut photo = Vec::with_capacity((cols * rows) as usize);
        for by in 0..rows {
            for bx in 0..cols {
                let x0 = bx * BLOCK;
                let y0 = by * BLOCK;
                let x1 = (x0 + BLOCK).min(px.width);
                let y1 = (y0 + BLOCK).min(px.height);
                // Partial blocks on the right/bottom edge need proportionally fewer colors.
                let pixels = (x1 - x0) * (y1 - y0);
                let scaled = |n: u32| (n * pixels / (BLOCK * BLOCK)).max(1);
                let (distinct, dominant) = set.count_block(px, x0, y0, x1, y1);
                photo.push(
                    distinct >= scaled(PHOTO_COLORS)
                        || (distinct >= scaled(MUTED_PHOTO_COLORS)
                            && dominant as f32 <= MAX_DOMINANT_SHARE * pixels as f32),
                );
            }
        }
        Self { cols, rows, photo }
    }

    fn get(&self, bx: i64, by: i64) -> bool {
        bx >= 0
            && by >= 0
            && (bx as u32) < self.cols
            && (by as u32) < self.rows
            && self.photo[(by as u32 * self.cols + bx as u32) as usize]
    }

    /// Fills a block that has photo blocks on both sides of it, horizontally or vertically.
    /// Photos often contain flat patches (sky, a wall, a dark background) that fall below
    /// the color threshold; this keeps them from splitting one image into several.
    fn closed(&self) -> Self {
        let mut photo = self.photo.clone();
        for by in 0..self.rows as i64 {
            for bx in 0..self.cols as i64 {
                let i = (by as u32 * self.cols + bx as u32) as usize;
                if !photo[i]
                    && ((self.get(bx - 1, by) && self.get(bx + 1, by))
                        || (self.get(bx, by - 1) && self.get(bx, by + 1)))
                {
                    photo[i] = true;
                }
            }
        }
        Self {
            cols: self.cols,
            rows: self.rows,
            photo,
        }
    }

    /// Bounding boxes (in pixels) of the 4-connected groups of photo blocks.
    fn components(&self) -> Vec<Region> {
        let mut seen = vec![false; self.photo.len()];
        let mut stack = Vec::new();
        let mut out = Vec::new();
        for start in 0..self.photo.len() {
            if !self.photo[start] || seen[start] {
                continue;
            }
            seen[start] = true;
            stack.push(start);
            let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0, 0);
            while let Some(i) = stack.pop() {
                let bx = i as u32 % self.cols;
                let by = i as u32 / self.cols;
                min_x = min_x.min(bx);
                max_x = max_x.max(bx);
                min_y = min_y.min(by);
                max_y = max_y.max(by);
                let neighbors = [
                    (bx as i64 - 1, by as i64),
                    (bx as i64 + 1, by as i64),
                    (bx as i64, by as i64 - 1),
                    (bx as i64, by as i64 + 1),
                ];
                for (nx, ny) in neighbors {
                    if self.get(nx, ny) {
                        let j = (ny as u32 * self.cols + nx as u32) as usize;
                        if !seen[j] {
                            seen[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
            out.push(Region {
                x: min_x * BLOCK,
                y: min_y * BLOCK,
                width: (max_x + 1 - min_x) * BLOCK,
                height: (max_y + 1 - min_y) * BLOCK,
            });
        }
        out
    }

    /// Share of `r`'s area covered by photo blocks, counting partial overlaps.
    fn photo_fill(&self, r: &Region) -> f32 {
        let bx0 = r.x / BLOCK;
        let by0 = r.y / BLOCK;
        let bx1 = (r.right() - 1) / BLOCK;
        let by1 = (r.bottom() - 1) / BLOCK;
        let mut covered = 0u64;
        for by in by0..=by1 {
            for bx in bx0..=bx1 {
                if self.photo[(by * self.cols + bx) as usize] {
                    let ox = (r.right().min((bx + 1) * BLOCK)) - r.x.max(bx * BLOCK);
                    let oy = (r.bottom().min((by + 1) * BLOCK)) - r.y.max(by * BLOCK);
                    covered += ox as u64 * oy as u64;
                }
            }
        }
        covered as f32 / r.area() as f32
    }
}

/// Trims and splits a box on uniform lines. A block-aligned component box comes in grown by
/// one block on every side first, because an image's edge blocks are often only partly
/// covered and so not colorful enough to be marked; the gutter cuts then shave off whatever
/// page content the margin picked up.
fn refine(px: &Pixels, r: Region, depth: u32, out: &mut Vec<Region>) {
    let margin = if depth == 0 { BLOCK } else { 0 };
    let mut x0 = r.x.saturating_sub(margin);
    let mut y0 = r.y.saturating_sub(margin);
    let mut x1 = (r.right() + margin).min(px.width);
    let mut y1 = (r.bottom() + margin).min(px.height);

    // Trim uniform edges: page background, borders, letterboxing.
    while y0 < y1 && px.row_uniform(y0, x0, x1) {
        y0 += 1;
    }
    while y1 > y0 && px.row_uniform(y1 - 1, x0, x1) {
        y1 -= 1;
    }
    // The column scans below read row `y0`, which is out of bounds once every row is gone.
    if y1 - y0 < MIN_SIDE {
        return;
    }
    while x0 < x1 && px.col_uniform(x0, y0, y1) {
        x0 += 1;
    }
    while x1 > x0 && px.col_uniform(x1 - 1, y0, y1) {
        x1 -= 1;
    }
    if x1 - x0 < MIN_SIDE || y1 - y0 < MIN_SIDE {
        return;
    }

    if depth < MAX_CUT_DEPTH {
        // Split on the first interior gutter. Each half is trimmed again on recursion, so
        // the rest of the gutter falls away there.
        if let Some(y) = (y0 + 1..y1 - 1).find(|&y| px.row_uniform(y, x0, x1)) {
            refine(
                px,
                Region {
                    x: x0,
                    y: y0,
                    width: x1 - x0,
                    height: y - y0,
                },
                depth + 1,
                out,
            );
            refine(
                px,
                Region {
                    x: x0,
                    y,
                    width: x1 - x0,
                    height: y1 - y,
                },
                depth + 1,
                out,
            );
            return;
        }
        if let Some(x) = (x0 + 1..x1 - 1).find(|&x| px.col_uniform(x, y0, y1)) {
            refine(
                px,
                Region {
                    x: x0,
                    y: y0,
                    width: x - x0,
                    height: y1 - y0,
                },
                depth + 1,
                out,
            );
            refine(
                px,
                Region {
                    x,
                    y: y0,
                    width: x1 - x,
                    height: y1 - y0,
                },
                depth + 1,
                out,
            );
            return;
        }
    }

    out.push(Region {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    });
}

/// Small open-addressing counter of packed RGB values, reused across blocks. A generation
/// stamp per slot means starting a new block is O(1) instead of clearing the table.
struct ColorSet {
    keys: [u32; 512],
    counts: [u16; 512],
    stamps: [u32; 512],
    generation: u32,
}

impl ColorSet {
    fn new() -> Self {
        Self {
            keys: [0; 512],
            counts: [0; 512],
            stamps: [0; 512],
            generation: 0,
        }
    }

    /// Distinct colors in the block, capped at [`COLOR_CAP`], and how many pixels the most
    /// common one covers (only exact when the cap isn't reached).
    fn count_block(&mut self, px: &Pixels, x0: u32, y0: u32, x1: u32, y1: u32) -> (u32, u32) {
        self.generation += 1;
        let mut count = 0;
        let mut dominant = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let [r, g, b] = px.rgb(x, y);
                let key = (r as u32) << 16 | (g as u32) << 8 | b as u32;
                let mut slot = (key.wrapping_mul(0x9E37_79B1) >> 23) as usize;
                loop {
                    if self.stamps[slot] != self.generation {
                        self.stamps[slot] = self.generation;
                        self.keys[slot] = key;
                        self.counts[slot] = 1;
                        dominant = dominant.max(1);
                        count += 1;
                        if count >= COLOR_CAP {
                            return (count, dominant);
                        }
                        break;
                    }
                    if self.keys[slot] == key {
                        self.counts[slot] += 1;
                        dominant = dominant.max(self.counts[slot] as u32);
                        break;
                    }
                    slot = (slot + 1) & 511;
                }
            }
        }
        (count, dominant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    const WHITE: Rgb<u8> = Rgb([255, 255, 255]);

    /// Fills a rectangle with smooth, noisy color, like a photo.
    fn paint_photo(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, seed: u32) {
        let mut state = seed.wrapping_mul(2_654_435_761).max(1);
        for py in y..y + h {
            for px in x..x + w {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let noise = (state % 24) as u8;
                let r = ((px - x) * 200 / w) as u8 + noise;
                let g = ((py - y) * 200 / h) as u8 + noise;
                img.put_pixel(px, py, Rgb([r, g, 120 + noise]));
            }
        }
    }

    /// Two-color "text" lines: dense edges but few colors.
    fn paint_text(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32) {
        for py in y..y + h {
            for px in x..x + w {
                if (px / 3 + py / 2) % 3 == 0 && py % 14 < 10 {
                    img.put_pixel(px, py, Rgb([20, 20, 20]));
                }
            }
        }
    }

    fn page() -> RgbImage {
        RgbImage::from_pixel(1280, 720, WHITE)
    }

    #[test]
    fn finds_a_single_photo_with_exact_bounds() {
        let mut img = page();
        paint_text(&mut img, 40, 40, 400, 600);
        paint_photo(&mut img, 503, 117, 401, 299, 1);
        let regions = extract_regions(&DynamicImage::ImageRgb8(img), 6);
        assert_eq!(
            regions,
            vec![Region {
                x: 503,
                y: 117,
                width: 401,
                height: 299
            }]
        );
    }

    #[test]
    fn text_and_flat_ui_yield_nothing() {
        let mut img = page();
        paint_text(&mut img, 0, 0, 1280, 720);
        for y in 100..200 {
            for x in 100..600 {
                img.put_pixel(x, y, Rgb([30, 90, 200]));
            }
        }
        assert!(extract_regions(&DynamicImage::ImageRgb8(img), 6).is_empty());
    }

    #[test]
    fn splits_images_separated_by_a_thin_gutter() {
        let mut img = page();
        // Two photos 4px apart, which lands the gutter inside one 16px block.
        paint_photo(&mut img, 100, 100, 300, 250, 1);
        paint_photo(&mut img, 404, 100, 300, 250, 2);
        let mut regions = extract_regions(&DynamicImage::ImageRgb8(img), 6);
        regions.sort_by_key(|r| r.x);
        assert_eq!(
            regions,
            vec![
                Region {
                    x: 100,
                    y: 100,
                    width: 300,
                    height: 250
                },
                Region {
                    x: 404,
                    y: 100,
                    width: 300,
                    height: 250
                },
            ]
        );
    }

    #[test]
    fn keeps_only_the_largest_ones() {
        let mut img = page();
        // A 4×2 grid of thumbnails, with the last two smaller than the rest.
        for i in 0..8u32 {
            let size = if i < 6 { 140 } else { 100 };
            paint_photo(
                &mut img,
                20 + (i % 4) * 160,
                20 + (i / 4) * 200,
                size,
                size,
                i + 1,
            );
        }
        let regions = extract_regions(&DynamicImage::ImageRgb8(img), 6);
        assert_eq!(regions.len(), 6);
        assert!(regions.iter().all(|r| r.width == 140 && r.height == 140));
    }

    #[test]
    fn drops_photos_smaller_than_min_side() {
        let mut img = page();
        paint_photo(&mut img, 100, 100, MIN_SIDE - 8, 200, 1);
        assert!(extract_regions(&DynamicImage::ImageRgb8(img), 6).is_empty());
    }

    #[test]
    fn full_screen_photo_is_one_region() {
        let mut img = page();
        paint_photo(&mut img, 0, 0, 1280, 720, 3);
        let regions = extract_regions(&DynamicImage::ImageRgb8(img), 6);
        assert_eq!(
            regions,
            vec![Region {
                x: 0,
                y: 0,
                width: 1280,
                height: 720
            }]
        );
    }
}
