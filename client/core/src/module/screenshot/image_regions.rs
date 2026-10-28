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
//!    uniform rows and columns from the edges, and split on gutters running all the way
//!    across, which separates images laid out in a grid. A gutter may have a few odd pixels,
//!    so a scrollbar or window border crossing it doesn't stop the split.
//! 4. Cut large boxes with no gutter along long straight edges, over just the edge's
//!    length: a photo wallpaper otherwise merges with the window in front of it into one
//!    screen-sized box. Then find the colorful areas again from only the blocks inside each
//!    piece, since an image in a window often touched the wallpaper through the window's
//!    edge. Large, roughly square pieces count as windows (see [`Extracted::container`]).
//! 5. Drop pieces that are too small, not mostly colorful, or duplicates, and keep the
//!    largest few.
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
/// See [`ExtractParams::gutter_slack`].
const GUTTER_SLACK: u32 = 4;
/// Pieces narrower or shorter than this are dropped: upscaled to 224×224 they give the
/// model too little to go on, and they're mostly avatars and icons.
pub const MIN_SIDE: u32 = 64;
/// A piece must be at least this share photo-like blocks, measured before hole-closing.
const MIN_PHOTO_FILL: f32 = 0.4;
/// XY-cut recursion limit; a grid of thumbnails needs one level per row/column split.
const MAX_CUT_DEPTH: u32 = 8;
/// Edge cuts are only tried in boxes covering at least this share of the screen. The case
/// they're for is a wallpaper merged with a window, and inside smaller boxes a straight edge
/// is more likely to be part of a photo than a boundary between two things.
const EDGE_SPLIT_MIN_SHARE: f32 = 0.15;
/// Neighboring pixels this different (largest channel difference) form an edge.
const EDGE_CONTRAST: i32 = 20;
/// An edge must run straight for at least this many pixels to be cut along.
const MIN_EDGE_RUN: u32 = 160;
/// Breaks up to this long (where the two sides happen to match) don't end an edge.
const EDGE_GAP: u32 = 6;
/// An edge-cut piece counts as a window (see [`Extracted::container`]) if it covers at least
/// this percent of the screen, and is no more than this many times longer than it is wide.
/// Smaller pieces come from cuts along an image's own border, and a thin strip is a sliver
/// of a window rather than one.
const WINDOW_MIN_SHARE: u64 = 10;
const WINDOW_MAX_ASPECT: u32 = 3;

/// The tunable thresholds of [`extract_regions_with`]. [`Default`] gives the values
/// [`extract_regions`] uses; the evaluation tooling overrides them to experiment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtractParams {
    pub photo_colors: u32,
    pub muted_photo_colors: u32,
    pub max_dominant_share: f32,
    pub gutter_tolerance: i32,
    /// Percent of a gutter row or column allowed to differ, so a scrollbar or window border
    /// crossing it doesn't stop the split. 0 means every pixel must match.
    pub gutter_slack: u32,
    pub min_side: u32,
    pub min_photo_fill: f32,
    pub edge_contrast: i32,
    /// 0 turns edge cuts off.
    pub min_edge_run: u32,
    pub edge_gap: u32,
}

impl Default for ExtractParams {
    fn default() -> Self {
        Self {
            photo_colors: PHOTO_COLORS,
            muted_photo_colors: MUTED_PHOTO_COLORS,
            max_dominant_share: MAX_DOMINANT_SHARE,
            gutter_tolerance: GUTTER_TOLERANCE,
            gutter_slack: GUTTER_SLACK,
            min_side: MIN_SIDE,
            min_photo_fill: MIN_PHOTO_FILL,
            edge_contrast: EDGE_CONTRAST,
            min_edge_run: MIN_EDGE_RUN,
            edge_gap: EDGE_GAP,
        }
    }
}

/// An axis-aligned rectangle in screenshot pixel coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

/// A region and the window it was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extracted {
    pub region: Region,
    /// The window the region was found in: the smallest box an edge cut separated out
    /// around it, or the whole screen. An image filling much of a small browser window is
    /// prominent even though it covers little of the screen.
    pub container: Region,
}

/// Returns up to `max` photo-like regions of `img`, largest first.
pub fn extract_regions(img: &DynamicImage, max: usize) -> Vec<Region> {
    extract_regions_with(img, max, &ExtractParams::default())
}

/// [`extract_regions`] with explicit thresholds.
pub fn extract_regions_with(img: &DynamicImage, max: usize, params: &ExtractParams) -> Vec<Region> {
    extract_regions_tagged(img, max, params)
        .into_iter()
        .map(|e| e.region)
        .collect()
}

/// [`extract_regions_with`], also saying which window each region was found in.
pub fn extract_regions_tagged(
    img: &DynamicImage,
    max: usize,
    params: &ExtractParams,
) -> Vec<Extracted> {
    // Screenshots decode to RGBA8 or RGB8; read those in place rather than paying for a
    // full-frame conversion. Anything else is rare enough to just convert.
    let converted;
    let px = match img {
        DynamicImage::ImageRgba8(buf) => {
            Pixels::new(buf.as_raw(), buf.width(), buf.height(), 4, params)
        }
        DynamicImage::ImageRgb8(buf) => {
            Pixels::new(buf.as_raw(), buf.width(), buf.height(), 3, params)
        }
        other => {
            converted = other.to_rgb8();
            Pixels::new(
                converted.as_raw(),
                converted.width(),
                converted.height(),
                3,
                params,
            )
        }
    };
    let (width, height) = img.dimensions();
    if width < params.min_side || height < params.min_side {
        return Vec::new();
    }

    let grid = BlockGrid::classify(&px, params);
    let closed = grid.closed();

    let screen = Region {
        x: 0,
        y: 0,
        width,
        height,
    };
    let mut out = Refined {
        container: screen,
        ..Refined::default()
    };
    for component in closed.components() {
        refine(&px, &closed, component, 0, params, &mut out);
    }

    let big = |r: &Region| r.width >= params.min_side && r.height >= params.min_side;
    let colorful = |r: &Region| grid.photo_fill(r) >= params.min_photo_fill;
    let mut tagged: Vec<Extracted> = out
        .pieces
        .iter()
        .zip(&out.containers)
        .filter(|(r, _)| big(r) && colorful(r))
        .map(|(r, c)| Extracted {
            region: *r,
            container: *c,
        })
        .collect();

    // Re-finding images inside edge-cut pieces can reach the same image twice; one copy
    // must not count as two agreeing images.
    tagged.sort_by_key(|e| std::cmp::Reverse(e.region.area()));
    let mut unique: Vec<Extracted> = Vec::with_capacity(tagged.len());
    for e in tagged {
        if !unique.iter().any(|u| overlap(&u.region, &e.region) > 0.9) {
            unique.push(e);
        }
    }
    unique.truncate(max);
    unique
}

/// What `refine` produces: the final pieces with the window each was found in.
#[derive(Default)]
struct Refined {
    pieces: Vec<Region>,
    containers: Vec<Region>,
    /// The window `refine` is currently working inside.
    container: Region,
}

/// Intersection over union of two regions.
fn overlap(a: &Region, b: &Region) -> f32 {
    let w = a.right().min(b.right()).saturating_sub(a.x.max(b.x));
    let h = a.bottom().min(b.bottom()).saturating_sub(a.y.max(b.y));
    let inter = w as u64 * h as u64;
    inter as f32 / (a.area() + b.area() - inter) as f32
}

/// Borrowed view of an 8-bit RGB or RGBA buffer.
struct Pixels<'a> {
    data: &'a [u8],
    width: u32,
    height: u32,
    channels: usize,
    gutter_tolerance: i32,
    gutter_slack: u32,
}

impl<'a> Pixels<'a> {
    fn new(
        data: &'a [u8],
        width: u32,
        height: u32,
        channels: usize,
        params: &ExtractParams,
    ) -> Self {
        Self {
            data,
            width,
            height,
            channels,
            gutter_tolerance: params.gutter_tolerance,
            gutter_slack: params.gutter_slack,
        }
    }

    #[inline]
    fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * self.width as usize + x as usize) * self.channels;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    /// True if every pixel in row `y` between `x0..x1` matches the first one.
    fn row_uniform(&self, y: u32, x0: u32, x1: u32) -> bool {
        self.uniform(x1 - x0, |i| self.rgb(x0 + i, y))
    }

    /// True if every pixel in column `x` between `y0..y1` matches the first one.
    fn col_uniform(&self, x: u32, y0: u32, y1: u32) -> bool {
        self.uniform(y1 - y0, |i| self.rgb(x, y0 + i))
    }

    /// True if the `len` pixels `at(0..len)` match, allowing `gutter_slack` percent to
    /// differ. With slack, the reference is the middle pixel, since the odd ones out (a
    /// scrollbar, a border) sit at the ends.
    fn uniform(&self, len: u32, at: impl Fn(u32) -> [u8; 3]) -> bool {
        if len == 0 {
            return true;
        }
        if self.gutter_slack == 0 {
            let first = at(0);
            return (1..len).all(|i| self.close(first, at(i)));
        }
        let reference = at(len / 2);
        let allowed = len * self.gutter_slack / 100;
        let mut misses = 0;
        for i in 0..len {
            if !self.close(reference, at(i)) {
                misses += 1;
                if misses > allowed {
                    return false;
                }
            }
        }
        true
    }

    #[inline]
    fn close(&self, a: [u8; 3], b: [u8; 3]) -> bool {
        (0..3).all(|c| (a[c] as i32 - b[c] as i32).abs() <= self.gutter_tolerance)
    }
}

/// One flag per block: is it photo-like.
struct BlockGrid {
    cols: u32,
    rows: u32,
    photo: Vec<bool>,
}

impl BlockGrid {
    fn classify(px: &Pixels, params: &ExtractParams) -> Self {
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
                    distinct >= scaled(params.photo_colors)
                        || (distinct >= scaled(params.muted_photo_colors)
                            && dominant as f32 <= params.max_dominant_share * pixels as f32),
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
        self.components_in(0, 0, self.cols, self.rows)
    }

    /// Components of the photo blocks lying entirely inside `r`.
    fn components_within(&self, r: &Region) -> Vec<Region> {
        let bx0 = r.x.div_ceil(BLOCK);
        let by0 = r.y.div_ceil(BLOCK);
        let bx1 = (r.right() / BLOCK).min(self.cols);
        let by1 = (r.bottom() / BLOCK).min(self.rows);
        if bx0 >= bx1 || by0 >= by1 {
            return Vec::new();
        }
        self.components_in(bx0, by0, bx1, by1)
    }

    /// Bounding boxes of the 4-connected groups of photo blocks within block columns
    /// `bx0..bx1` and rows `by0..by1`.
    fn components_in(&self, bx0: u32, by0: u32, bx1: u32, by1: u32) -> Vec<Region> {
        let inside = |bx: i64, by: i64| {
            bx >= bx0 as i64 && by >= by0 as i64 && bx < bx1 as i64 && by < by1 as i64
        };
        let mut seen = vec![false; self.photo.len()];
        let mut stack = Vec::new();
        let mut out = Vec::new();
        for by in by0..by1 {
            for bx in bx0..bx1 {
                let start = (by * self.cols + bx) as usize;
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
                        if inside(nx, ny) && self.get(nx, ny) {
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
fn refine(
    px: &Pixels,
    grid: &BlockGrid,
    r: Region,
    depth: u32,
    params: &ExtractParams,
    out: &mut Refined,
) {
    let min_side = params.min_side;
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
    if y1 - y0 < min_side {
        return;
    }
    while x0 < x1 && px.col_uniform(x0, y0, y1) {
        x0 += 1;
    }
    while x1 > x0 && px.col_uniform(x1 - 1, y0, y1) {
        x1 -= 1;
    }
    if x1 - x0 < min_side || y1 - y0 < min_side {
        return;
    }

    if depth < MAX_CUT_DEPTH {
        // Split on the first interior gutter. Each half is trimmed again on recursion, so
        // the rest of the gutter falls away there.
        if let Some(y) = (y0 + 1..y1 - 1).find(|&y| px.row_uniform(y, x0, x1)) {
            refine(
                px,
                grid,
                Region {
                    x: x0,
                    y: y0,
                    width: x1 - x0,
                    height: y - y0,
                },
                depth + 1,
                params,
                out,
            );
            refine(
                px,
                grid,
                Region {
                    x: x0,
                    y,
                    width: x1 - x0,
                    height: y1 - y,
                },
                depth + 1,
                params,
                out,
            );
            return;
        }
        if let Some(x) = (x0 + 1..x1 - 1).find(|&x| px.col_uniform(x, y0, y1)) {
            refine(
                px,
                grid,
                Region {
                    x: x0,
                    y: y0,
                    width: x - x0,
                    height: y1 - y0,
                },
                depth + 1,
                params,
                out,
            );
            refine(
                px,
                grid,
                Region {
                    x,
                    y: y0,
                    width: x1 - x,
                    height: y1 - y0,
                },
                depth + 1,
                params,
                out,
            );
            return;
        }
        let share = ((x1 - x0) as u64 * (y1 - y0) as u64) as f32
            / (px.width as u64 * px.height as u64) as f32;
        if params.min_edge_run > 0
            && share >= EDGE_SPLIT_MIN_SHARE
            && let Some(pieces) = edge_cut(px, x0, y0, x1, y1, params)
        {
            // An image inside a window is often joined to the wallpaper only through blocks
            // along the window's edge. Finding the colorful areas again, from just the
            // blocks inside each piece, lets it stand on its own.
            let outer = out.container;
            let screen = px.width as u64 * px.height as u64;
            let window = |r: &Region| {
                r.area() * 100 >= screen * WINDOW_MIN_SHARE
                    && r.width.max(r.height) <= WINDOW_MAX_ASPECT * r.width.min(r.height)
            };
            for piece in pieces {
                out.container = if window(&piece) { piece } else { outer };
                for sub in grid.components_within(&piece) {
                    let x0 = sub.x.saturating_sub(BLOCK).max(piece.x);
                    let y0 = sub.y.saturating_sub(BLOCK).max(piece.y);
                    let x1 = (sub.right() + BLOCK).min(piece.right());
                    let y1 = (sub.bottom() + BLOCK).min(piece.bottom());
                    let grown = Region {
                        x: x0,
                        y: y0,
                        width: x1 - x0,
                        height: y1 - y0,
                    };
                    refine(px, grid, grown, depth + 1, params, out);
                }
            }
            out.container = outer;
            return;
        }
    }

    out.pieces.push(Region {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    });
    out.containers.push(out.container);
}

/// Finds the longest straight vertical or horizontal edge in the box and cuts along it, over
/// the edge's length only. A vertical edge at `x` from `ya` to `yb` gives up to four boxes:
/// the full-width strips above `ya` and below `yb`, and the parts left and right of `x`
/// between them. A quarter-screen window's left edge thus separates the window from the
/// wallpaper beside it, and the strip below it.
fn edge_cut(
    px: &Pixels,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    params: &ExtractParams,
) -> Option<Vec<Region>> {
    let longest = |len: u32, along: &dyn Fn(u32, u32) -> bool, lines: std::ops::Range<u32>| {
        let mut best: Option<(u32, u32, u32)> = None; // (line, from, to)
        for line in lines {
            let mut start = None;
            let mut last = 0;
            let record = |from: u32, to: u32, best: &mut Option<(u32, u32, u32)>| {
                if best.is_none_or(|(_, f, t)| to - from > t - f) {
                    *best = Some((line, from, to));
                }
            };
            for i in 0..len {
                if along(line, i) {
                    start.get_or_insert(i);
                    last = i;
                } else if let Some(s) = start
                    && i - last > params.edge_gap
                {
                    record(s, last + 1, &mut best);
                    start = None;
                }
            }
            if let Some(s) = start {
                record(s, last + 1, &mut best);
            }
        }
        best
    };
    let differs = |a: [u8; 3], b: [u8; 3]| {
        (0..3).any(|c| (a[c] as i32 - b[c] as i32).abs() >= params.edge_contrast)
    };
    // Lines stay a minimum side away from the box's own borders, so every cut leaves pieces
    // big enough to keep.
    let m = params.min_side;
    let vertical = (x1 - x0 > 2 * m).then(|| {
        longest(
            y1 - y0,
            &|x, i| differs(px.rgb(x - 1, y0 + i), px.rgb(x, y0 + i)),
            x0 + m..x1 - m,
        )
    });
    let horizontal = (y1 - y0 > 2 * m).then(|| {
        longest(
            x1 - x0,
            &|y, i| differs(px.rgb(x0 + i, y - 1), px.rgb(x0 + i, y)),
            y0 + m..y1 - m,
        )
    });
    let run = |c: &Option<(u32, u32, u32)>| c.map_or(0, |(_, f, t)| t - f);
    let (v, h) = (vertical.flatten(), horizontal.flatten());
    let rect = |x: u32, y: u32, w: u32, h: u32| Region {
        x,
        y,
        width: w,
        height: h,
    };
    let mut pieces = Vec::new();
    if run(&v) >= run(&h) {
        let (x, ya, yb) = v?;
        if yb - ya < params.min_edge_run {
            return None;
        }
        let (ya, yb) = (y0 + ya, y0 + yb);
        pieces.push(rect(x0, y0, x1 - x0, ya - y0));
        pieces.push(rect(x0, ya, x - x0, yb - ya));
        pieces.push(rect(x, ya, x1 - x, yb - ya));
        pieces.push(rect(x0, yb, x1 - x0, y1 - yb));
    } else {
        let (y, xa, xb) = h?;
        if xb - xa < params.min_edge_run {
            return None;
        }
        let (xa, xb) = (x0 + xa, x0 + xb);
        pieces.push(rect(x0, y0, xa - x0, y1 - y0));
        pieces.push(rect(xa, y0, xb - xa, y - y0));
        pieces.push(rect(xa, y, xb - xa, y1 - y));
        pieces.push(rect(xb, y0, x1 - xb, y1 - y0));
    }
    pieces.retain(|r| r.width > 0 && r.height > 0);
    Some(pieces)
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
    fn separates_a_window_from_the_photo_wallpaper_behind_it() {
        // A photo wallpaper, and a quarter-screen window in the top right whose two
        // thumbnails sit 6px from its edges, close enough to merge with the wallpaper's
        // blocks. The window is one flat page color with a 1px border.
        let mut img = page();
        paint_photo(&mut img, 0, 0, 1280, 720, 9);
        for y in 0..360 {
            for x in 640..1280 {
                let border = x == 640 || y == 359;
                let c = if border { 60 } else { 245 };
                img.put_pixel(x, y, Rgb([c, c, c]));
            }
        }
        paint_photo(&mut img, 647, 40, 300, 200, 1);
        paint_photo(&mut img, 960, 40, 300, 200, 2);
        let regions = extract_regions(&DynamicImage::ImageRgb8(img), 6);
        for thumb in [
            Region {
                x: 647,
                y: 40,
                width: 300,
                height: 200,
            },
            Region {
                x: 960,
                y: 40,
                width: 300,
                height: 200,
            },
        ] {
            assert!(regions.contains(&thumb), "{thumb:?} not in {regions:?}");
        }
    }

    #[test]
    fn finds_a_large_image_inside_a_small_window() {
        // An eighth-screen window in the bottom right of a photo wallpaper, holding one large
        // image 4px from the window's sides under a flat header, like an opened video.
        let mut img = page();
        paint_photo(&mut img, 0, 0, 1280, 720, 9);
        for y in 360..720 {
            for x in 880..1280 {
                let border = x == 880 || y == 360;
                let c = if border { 60 } else { 30 };
                img.put_pixel(x, y, Rgb([c, c, c]));
            }
        }
        paint_photo(&mut img, 885, 420, 391, 220, 1);
        let regions =
            extract_regions_tagged(&DynamicImage::ImageRgb8(img), 12, &ExtractParams::default());
        let video = Region {
            x: 885,
            y: 420,
            width: 391,
            height: 220,
        };
        let found = regions.iter().find(|e| e.region == video);
        let found = found.unwrap_or_else(|| panic!("{video:?} not in {regions:?}"));
        // It was found inside the window, not the whole screen.
        assert!(
            found.container.width < 1280 && found.container.height < 720,
            "{found:?}"
        );
    }

    #[test]
    fn splits_on_a_gutter_crossed_by_a_scrollbar() {
        // Two photos stacked 8px apart, both running into a scrollbar whose thumb breaks
        // every column, so only the row between them can split them, and that row crosses
        // the scrollbar track.
        let mut img = page();
        paint_photo(&mut img, 100, 100, 304, 200, 1);
        paint_photo(&mut img, 100, 308, 304, 200, 2);
        for y in 60..560 {
            let c = if (120..260).contains(&y) { 120 } else { 225 };
            for x in 404..416 {
                img.put_pixel(x, y, Rgb([c, c, c]));
            }
        }
        let img = DynamicImage::ImageRgb8(img);
        // Edge cuts could also split along the photos' borders; leave them out.
        let slack = ExtractParams {
            min_edge_run: 0,
            ..ExtractParams::default()
        };
        let strict = ExtractParams {
            gutter_slack: 0,
            ..slack
        };
        assert_eq!(extract_regions_with(&img, 12, &strict).len(), 1);

        let mut regions = extract_regions_with(&img, 12, &slack);
        regions.sort_by_key(|r| r.y);
        assert_eq!(regions.len(), 2, "{regions:?}");
        assert_eq!((regions[0].y, regions[0].height), (100, 200), "{regions:?}");
        assert_eq!((regions[1].y, regions[1].height), (308, 200), "{regions:?}");
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
