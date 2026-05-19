use image::{DynamicImage, GenericImageView, Rgba};
use std::collections::{HashSet, VecDeque};

// Fine tile size — smaller = more precise image boundaries, slower scoring.
const FINE_TILE_PX: u32 = 32;
// A tile with score >= this is considered "image content".
const HOT_THRESHOLD: f32 = 1.5;
// Discard patches smaller than this many original-hot tiles (eliminates isolated colored UI elements).
const MIN_PATCH_TILES: usize = 4;
// Maximum patches to run the NSFW classifier on.
const MAX_REGIONS: usize = 4;
// Dilate the hot mask by this many tiles before BFS, bridging thin cold gaps inside a photo.
const DILATION_RADIUS: usize = 2;

// Scoring constants
const COLOR_BUCKET_DIVISOR: u8 = 32; // 8 levels/channel → 8³=512 buckets
const PIXEL_SAMPLE_STRIDE: u32 = 3;
const GRADIENT_NORMALIZER: f64 = 50.0;
// Pixels with luma in [MID_LUMA_LO, MID_LUMA_HI] are "mid-range" — not extreme white or black.
// Photos and drawings have many mid-range pixels; text-on-white has very few.
const MID_LUMA_LO: f64 = 20.0;
const MID_LUMA_HI: f64 = 225.0;
// At MID_FRAC_LO the weight is 0; at MID_FRAC_HI it reaches 1 (no penalty).
const MID_FRAC_LO: f64 = 0.08;
const MID_FRAC_HI: f64 = 0.45;

// score = color_diversity × √luma_variance / (1 + grad_mean / GRADIENT_NORMALIZER)
//         × mid_luma_weight
//
// mid_luma_weight penalises tiles that are mostly near-white (text-on-white UI).
// Photos/drawings have pixels spread across the luma range; text tiles are mostly white.
fn tile_image_score(img: &DynamicImage) -> f32 {
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());
    if w == 0 || h == 0 {
        return 0.0;
    }

    let mut luma_sum = 0.0f64;
    let mut luma_sq_sum = 0.0f64;
    let mut grad_sum = 0.0f64;
    let mut grad_sq_sum = 0.0f64;
    let mut grad_count = 0u64;
    let mut mid_luma_count = 0u64;
    let mut color_buckets: HashSet<u16> = HashSet::new();

    for y in 0..h {
        for x in 0..w {
            let p = rgb.get_pixel(x, y);
            let (r, g, b) = (p.0[0] as f64, p.0[1] as f64, p.0[2] as f64);

            let luma = 0.299 * r + 0.587 * g + 0.114 * b;
            luma_sum += luma;
            luma_sq_sum += luma * luma;

            if luma >= MID_LUMA_LO && luma <= MID_LUMA_HI {
                mid_luma_count += 1;
            }

            if (x + y * w) % PIXEL_SAMPLE_STRIDE == 0 {
                let br = (p.0[0] / COLOR_BUCKET_DIVISOR) as u16;
                let bg = (p.0[1] / COLOR_BUCKET_DIVISOR) as u16;
                let bb = (p.0[2] / COLOR_BUCKET_DIVISOR) as u16;
                color_buckets.insert(br * 64 + bg * 8 + bb);
            }

            if x + 1 < w {
                let p2 = rgb.get_pixel(x + 1, y);
                let diff = ((p2.0[0] as f64 - r).abs()
                    + (p2.0[1] as f64 - g).abs()
                    + (p2.0[2] as f64 - b).abs())
                    / 3.0;
                grad_sum += diff;
                grad_sq_sum += diff * diff;
                grad_count += 1;
            }
        }
    }

    let n = (w * h) as f64;
    let luma_mean = luma_sum / n;
    let luma_variance = (luma_sq_sum / n - luma_mean * luma_mean).max(0.0);
    let grad_mean = if grad_count > 0 {
        grad_sum / grad_count as f64
    } else {
        0.0
    };
    let color_diversity = color_buckets.len() as f64 / 512.0;
    let mid_fraction = mid_luma_count as f64 / n;
    let mid_weight = ((mid_fraction - MID_FRAC_LO) / (MID_FRAC_HI - MID_FRAC_LO)).clamp(0.0, 1.0);

    // Gradient coefficient of variation: text has bimodal gradients (0 inside strokes, large at
    // edges) → high CV. Photos have a smoother distribution → lower CV.
    let grad_var = (grad_sq_sum / grad_count.max(1) as f64)
        - (grad_mean * grad_mean);
    let grad_cv = grad_var.sqrt() / (grad_mean + 1.0);

    (color_diversity * luma_variance.sqrt()
        / (1.0 + grad_mean / GRADIENT_NORMALIZER)
        / (1.0 + grad_cv)
        * mid_weight) as f32
}

struct Patch {
    /// Pixel-coordinate bounding box: (x, y, width, height)
    bbox: (u32, u32, u32, u32),
    mean_score: f32,
    tile_count: usize,
}

/// BFS over the hot-tile grid (8-connectivity) to find connected image regions.
/// The hot mask is dilated by `dilation` tiles before BFS so thin cold gaps inside a photo
/// (dark caption strips, thin margins) don't split it into multiple patches.
/// MIN_PATCH_TILES and mean_score are computed against original (undilated) hot tiles only.
fn find_patches(
    scores: &[Vec<f32>],
    rows: usize,
    cols: usize,
    tile_w: u32,
    tile_h: u32,
    dilation: usize,
) -> Vec<Patch> {
    // Original hot mask
    let orig_hot: Vec<Vec<bool>> = (0..rows)
        .map(|r| (0..cols).map(|c| scores[r][c] >= HOT_THRESHOLD).collect())
        .collect();

    // Dilated mask: every tile within `dilation` of a hot tile is also hot
    let mut dilated = vec![vec![false; cols]; rows];
    for r in 0..rows {
        for c in 0..cols {
            if orig_hot[r][c] {
                let r0 = r.saturating_sub(dilation);
                let r1 = (r + dilation + 1).min(rows);
                let c0 = c.saturating_sub(dilation);
                let c1 = (c + dilation + 1).min(cols);
                for nr in r0..r1 {
                    for nc in c0..c1 {
                        dilated[nr][nc] = true;
                    }
                }
            }
        }
    }

    let mut visited = vec![vec![false; cols]; rows];
    let mut patches = Vec::new();

    for start_r in 0..rows {
        for start_c in 0..cols {
            if visited[start_r][start_c] || !dilated[start_r][start_c] {
                visited[start_r][start_c] = true;
                continue;
            }

            // BFS on dilated mask; track which tiles are orig-hot separately
            let mut orig_in_component: Vec<(usize, usize)> = Vec::new();
            let mut all_in_component: Vec<(usize, usize)> = Vec::new();
            let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
            queue.push_back((start_r, start_c));
            visited[start_r][start_c] = true;

            while let Some((r, c)) = queue.pop_front() {
                all_in_component.push((r, c));
                if orig_hot[r][c] {
                    orig_in_component.push((r, c));
                }
                for dr in -1i32..=1 {
                    for dc in -1i32..=1 {
                        if dr == 0 && dc == 0 {
                            continue;
                        }
                        let nr = r as i32 + dr;
                        let nc = c as i32 + dc;
                        if nr < 0 || nr >= rows as i32 || nc < 0 || nc >= cols as i32 {
                            continue;
                        }
                        let (nr, nc) = (nr as usize, nc as usize);
                        if !visited[nr][nc] && dilated[nr][nc] {
                            visited[nr][nc] = true;
                            queue.push_back((nr, nc));
                        }
                    }
                }
            }

            if orig_in_component.len() < MIN_PATCH_TILES {
                continue;
            }

            // Bounding box from all (dilated) tiles so the crop includes thin borders
            let min_r = all_in_component.iter().map(|(r, _)| *r).min().unwrap();
            let max_r = all_in_component.iter().map(|(r, _)| *r).max().unwrap();
            let min_c = all_in_component.iter().map(|(_, c)| *c).min().unwrap();
            let max_c = all_in_component.iter().map(|(_, c)| *c).max().unwrap();

            let x = min_c as u32 * tile_w;
            let y = min_r as u32 * tile_h;
            let w = (max_c as u32 - min_c as u32 + 1) * tile_w;
            let h = (max_r as u32 - min_r as u32 + 1) * tile_h;

            let mean_score = orig_in_component
                .iter()
                .map(|(r, c)| scores[*r][*c])
                .sum::<f32>()
                / orig_in_component.len() as f32;

            patches.push(Patch {
                bbox: (x, y, w, h),
                mean_score,
                tile_count: orig_in_component.len(),
            });
        }
    }

    patches
}

fn fill_rect_blend(
    img: &mut image::RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: [u8; 3],
    alpha: f32,
) {
    let iw = img.width();
    let ih = img.height();
    for py in y..(y + h).min(ih) {
        for px in x..(x + w).min(iw) {
            let bg = img.get_pixel(px, py);
            let r = (bg.0[0] as f32 * (1.0 - alpha) + color[0] as f32 * alpha) as u8;
            let g = (bg.0[1] as f32 * (1.0 - alpha) + color[1] as f32 * alpha) as u8;
            let b = (bg.0[2] as f32 * (1.0 - alpha) + color[2] as f32 * alpha) as u8;
            img.put_pixel(px, py, Rgba([r, g, b, 255]));
        }
    }
}

// Edge-detection approach: compute Sobel-like gradient per coarse tile, threshold to a binary
// edge mask, dilate, find connected components → bounding boxes of "rectangles" in the layout.
// Then score each rectangle's interior with `tile_image_score` to keep only the image-like ones.
// When `debug_prefix` is Some, saves intermediate stage images as <prefix>_stageN.png.
fn edge_based_patches(img: &DynamicImage, debug_prefix: Option<&str>) -> Vec<Patch> {
    const EDGE_TILE_PX: u32 = 16;
    const EDGE_THRESHOLD: f32 = 12.0;
    const EDGE_DILATION: usize = 2;
    const MIN_RECT_TILES: usize = 16; // ≥ 16 edge-tiles in component → ~256² pixel region floor

    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());
    let edge_cols = ((w + EDGE_TILE_PX - 1) / EDGE_TILE_PX) as usize;
    let edge_rows = ((h + EDGE_TILE_PX - 1) / EDGE_TILE_PX) as usize;

    // Per-tile mean gradient magnitude (combined horizontal + vertical neighbour diffs)
    let mut edge_mag = vec![vec![0.0f32; edge_cols]; edge_rows];
    for r in 0..edge_rows {
        for c in 0..edge_cols {
            let x0 = c as u32 * EDGE_TILE_PX;
            let y0 = r as u32 * EDGE_TILE_PX;
            let x1 = (x0 + EDGE_TILE_PX).min(w);
            let y1 = (y0 + EDGE_TILE_PX).min(h);
            let mut total = 0.0f32;
            let mut count = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = rgb.get_pixel(x, y);
                    if x + 1 < w {
                        let p_r = rgb.get_pixel(x + 1, y);
                        total += ((p_r.0[0] as i32 - p.0[0] as i32).abs()
                            + (p_r.0[1] as i32 - p.0[1] as i32).abs()
                            + (p_r.0[2] as i32 - p.0[2] as i32).abs())
                            as f32
                            / 3.0;
                        count += 1;
                    }
                    if y + 1 < h {
                        let p_d = rgb.get_pixel(x, y + 1);
                        total += ((p_d.0[0] as i32 - p.0[0] as i32).abs()
                            + (p_d.0[1] as i32 - p.0[1] as i32).abs()
                            + (p_d.0[2] as i32 - p.0[2] as i32).abs())
                            as f32
                            / 3.0;
                        count += 1;
                    }
                }
            }
            edge_mag[r][c] = if count > 0 { total / count as f32 } else { 0.0 };
        }
    }

    // Save stage 1: edge magnitude as grayscale
    if let Some(prefix) = debug_prefix {
        let mut dbg = image::RgbaImage::new(w, h);
        let mut max_mag = 0.0f32;
        for row in &edge_mag {
            for &v in row {
                if v > max_mag {
                    max_mag = v;
                }
            }
        }
        let scale = if max_mag > 0.0 { 255.0 / max_mag } else { 1.0 };
        for r in 0..edge_rows {
            for c in 0..edge_cols {
                let v = (edge_mag[r][c] * scale).clamp(0.0, 255.0) as u8;
                let x0 = c as u32 * EDGE_TILE_PX;
                let y0 = r as u32 * EDGE_TILE_PX;
                let x1 = (x0 + EDGE_TILE_PX).min(w);
                let y1 = (y0 + EDGE_TILE_PX).min(h);
                for y in y0..y1 {
                    for x in x0..x1 {
                        dbg.put_pixel(x, y, Rgba([v, v, v, 255]));
                    }
                }
            }
        }
        let p = format!("{}_stage1_edge_mag.png", prefix);
        dbg.save(&p).ok();
        println!("  [debug] saved {p} (max edge mag = {:.1})", max_mag);
    }

    // Threshold → binary edge mask
    let mut binary = vec![vec![false; edge_cols]; edge_rows];
    for r in 0..edge_rows {
        for c in 0..edge_cols {
            binary[r][c] = edge_mag[r][c] > EDGE_THRESHOLD;
        }
    }

    if let Some(prefix) = debug_prefix {
        let mut dbg = image::RgbaImage::new(w, h);
        for r in 0..edge_rows {
            for c in 0..edge_cols {
                let col = if binary[r][c] {
                    Rgba([255, 255, 255, 255])
                } else {
                    Rgba([0, 0, 0, 255])
                };
                let x0 = c as u32 * EDGE_TILE_PX;
                let y0 = r as u32 * EDGE_TILE_PX;
                let x1 = (x0 + EDGE_TILE_PX).min(w);
                let y1 = (y0 + EDGE_TILE_PX).min(h);
                for y in y0..y1 {
                    for x in x0..x1 {
                        dbg.put_pixel(x, y, col);
                    }
                }
            }
        }
        let p = format!("{}_stage2_binary.png", prefix);
        dbg.save(&p).ok();
        println!("  [debug] saved {p}");
    }

    // Dilate to close small gaps between adjacent edge pixels
    let mut dilated = vec![vec![false; edge_cols]; edge_rows];
    for r in 0..edge_rows {
        for c in 0..edge_cols {
            if binary[r][c] {
                let r0 = r.saturating_sub(EDGE_DILATION);
                let r1 = (r + EDGE_DILATION + 1).min(edge_rows);
                let c0 = c.saturating_sub(EDGE_DILATION);
                let c1 = (c + EDGE_DILATION + 1).min(edge_cols);
                for nr in r0..r1 {
                    for nc in c0..c1 {
                        dilated[nr][nc] = true;
                    }
                }
            }
        }
    }

    if let Some(prefix) = debug_prefix {
        let mut dbg = image::RgbaImage::new(w, h);
        for r in 0..edge_rows {
            for c in 0..edge_cols {
                let col = if dilated[r][c] {
                    Rgba([255, 255, 255, 255])
                } else {
                    Rgba([0, 0, 0, 255])
                };
                let x0 = c as u32 * EDGE_TILE_PX;
                let y0 = r as u32 * EDGE_TILE_PX;
                let x1 = (x0 + EDGE_TILE_PX).min(w);
                let y1 = (y0 + EDGE_TILE_PX).min(h);
                for y in y0..y1 {
                    for x in x0..x1 {
                        dbg.put_pixel(x, y, col);
                    }
                }
            }
        }
        let p = format!("{}_stage3_dilated.png", prefix);
        dbg.save(&p).ok();
        println!("  [debug] saved {p}");
    }

    // Connected components → candidate rectangles
    let mut visited = vec![vec![false; edge_cols]; edge_rows];
    let mut candidates: Vec<(u32, u32, u32, u32)> = Vec::new();
    let mut component_id = vec![vec![-1i32; edge_cols]; edge_rows];
    let mut next_id: i32 = 0;

    for start_r in 0..edge_rows {
        for start_c in 0..edge_cols {
            if visited[start_r][start_c] || !dilated[start_r][start_c] {
                visited[start_r][start_c] = true;
                continue;
            }

            let id = next_id;
            next_id += 1;
            let mut component: Vec<(usize, usize)> = Vec::new();
            let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
            queue.push_back((start_r, start_c));
            visited[start_r][start_c] = true;
            while let Some((r, c)) = queue.pop_front() {
                component.push((r, c));
                component_id[r][c] = id;
                for dr in -1i32..=1 {
                    for dc in -1i32..=1 {
                        if dr == 0 && dc == 0 {
                            continue;
                        }
                        let nr = r as i32 + dr;
                        let nc = c as i32 + dc;
                        if nr < 0
                            || nr >= edge_rows as i32
                            || nc < 0
                            || nc >= edge_cols as i32
                        {
                            continue;
                        }
                        let (nr, nc) = (nr as usize, nc as usize);
                        if !visited[nr][nc] && dilated[nr][nc] {
                            visited[nr][nc] = true;
                            queue.push_back((nr, nc));
                        }
                    }
                }
            }

            if component.len() < MIN_RECT_TILES {
                continue;
            }

            let min_r = component.iter().map(|(r, _)| *r).min().unwrap();
            let max_r = component.iter().map(|(r, _)| *r).max().unwrap();
            let min_c = component.iter().map(|(_, c)| *c).min().unwrap();
            let max_c = component.iter().map(|(_, c)| *c).max().unwrap();

            let x = min_c as u32 * EDGE_TILE_PX;
            let y = min_r as u32 * EDGE_TILE_PX;
            let rect_w = ((max_c - min_c + 1) as u32 * EDGE_TILE_PX).min(w.saturating_sub(x));
            let rect_h = ((max_r - min_r + 1) as u32 * EDGE_TILE_PX).min(h.saturating_sub(y));

            candidates.push((x, y, rect_w, rect_h));
        }
    }

    if let Some(prefix) = debug_prefix {
        let mut dbg = image::RgbaImage::new(w, h);
        // Distinct colours, cycling
        let palette: &[Rgba<u8>] = &[
            Rgba([255, 80, 80, 255]),
            Rgba([80, 255, 80, 255]),
            Rgba([80, 160, 255, 255]),
            Rgba([255, 200, 60, 255]),
            Rgba([220, 80, 220, 255]),
            Rgba([80, 220, 220, 255]),
            Rgba([255, 140, 60, 255]),
            Rgba([180, 180, 255, 255]),
        ];
        for r in 0..edge_rows {
            for c in 0..edge_cols {
                let col = if component_id[r][c] >= 0 {
                    palette[component_id[r][c] as usize % palette.len()]
                } else {
                    Rgba([0, 0, 0, 255])
                };
                let x0 = c as u32 * EDGE_TILE_PX;
                let y0 = r as u32 * EDGE_TILE_PX;
                let x1 = (x0 + EDGE_TILE_PX).min(w);
                let y1 = (y0 + EDGE_TILE_PX).min(h);
                for y in y0..y1 {
                    for x in x0..x1 {
                        dbg.put_pixel(x, y, col);
                    }
                }
            }
        }
        let p = format!("{}_stage4_components.png", prefix);
        dbg.save(&p).ok();
        println!(
            "  [debug] saved {p} ({} component(s) total, including small ones)",
            next_id
        );
    }

    println!(
        "Edge detection found {} candidate rectangle(s):",
        candidates.len()
    );

    // Stage 2: score each candidate rectangle's interior with the image-content heuristic
    let mut patches: Vec<Patch> = Vec::new();
    for (x, y, rect_w, rect_h) in candidates {
        let crop = img.crop_imm(x, y, rect_w, rect_h);
        let score = tile_image_score(&crop);
        let kept = score >= HOT_THRESHOLD;
        println!(
            "  rect at ({:4},{:4}) {}×{}  score={:.2}  {}",
            x,
            y,
            rect_w,
            rect_h,
            score,
            if kept { "✓ kept" } else { "✗ dropped" }
        );
        if kept {
            patches.push(Patch {
                bbox: (x, y, rect_w, rect_h),
                mean_score: score,
                tile_count: 0,
            });
        }
    }

    patches.sort_by(|a, b| b.mean_score.partial_cmp(&a.mean_score).unwrap());
    patches
}

fn draw_rect_border(
    img: &mut image::RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: Rgba<u8>,
    thickness: u32,
) {
    let iw = img.width();
    let ih = img.height();
    for t in 0..thickness {
        for px in x..(x + w).min(iw) {
            if y + t < ih {
                img.put_pixel(px, y + t, color);
            }
            if y + h > t + 1 && y + h - t - 1 < ih {
                img.put_pixel(px, y + h - t - 1, color);
            }
        }
        for py in y..(y + h).min(ih) {
            if x + t < iw {
                img.put_pixel(x + t, py, color);
            }
            if x + w > t + 1 && x + w - t - 1 < iw {
                img.put_pixel(x + w - t - 1, py, color);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: region_demo <image.png> [--nsfw] [--edge]");
        std::process::exit(1);
    }
    let path = &args[1];
    let run_nsfw = args.iter().any(|a| a == "--nsfw");
    let use_edge = args.iter().any(|a| a == "--edge");

    let img = image::open(path).expect("Failed to open image");
    let (width, height) = img.dimensions();

    println!("Image: {}×{}", width, height);

    let mut annotated = img.to_rgba8();
    let mut patches: Vec<Patch>;

    if use_edge {
        println!("Method: edge-detection → rectangle candidates → image-content filter\n");
        let dbg_prefix = path.trim_end_matches(".png").to_string() + "_edge";
        patches = edge_based_patches(&img, Some(&dbg_prefix));
        println!(
            "\nKept {} image-rectangle(s) after scoring:",
            patches.len()
        );
        for (i, p) in patches.iter().enumerate() {
            let (x, y, w, h) = p.bbox;
            println!(
                "  rect {}: {}×{} px at ({},{})  score={:.2}",
                i, w, h, x, y, p.mean_score
            );
        }
    } else {
        println!("Method: per-tile scoring → BFS on dilated hot mask\n");
        let cols = ((width + FINE_TILE_PX - 1) / FINE_TILE_PX) as usize;
        let rows = ((height + FINE_TILE_PX - 1) / FINE_TILE_PX) as usize;
        let tile_w = (width + cols as u32 - 1) / cols as u32;
        let tile_h = (height + rows as u32 - 1) / rows as u32;

        println!(
            "Fine grid: {}×{} ({} tiles, ~{}×{} px each)",
            cols,
            rows,
            cols * rows,
            tile_w,
            tile_h
        );

        // Score every fine tile
        let mut scores: Vec<Vec<f32>> = Vec::with_capacity(rows);
        for row in 0..rows {
            let mut row_scores = Vec::with_capacity(cols);
            for col in 0..cols {
                let x = col as u32 * tile_w;
                let y = row as u32 * tile_h;
                let actual_w = tile_w.min(width.saturating_sub(x));
                let actual_h = tile_h.min(height.saturating_sub(y));
                if actual_w == 0 || actual_h == 0 {
                    row_scores.push(0.0);
                    continue;
                }
                let tile = img.crop_imm(x, y, actual_w, actual_h);
                row_scores.push(tile_image_score(&tile));
            }
            scores.push(row_scores);
        }

        println!("\nHot tile map (score ≥ {HOT_THRESHOLD}, H=hot):");
        for row in 0..rows {
            let line: String = (0..cols)
                .map(|col| if scores[row][col] >= HOT_THRESHOLD { 'H' } else { '.' })
                .collect();
            println!("  {line}");
        }

        patches = find_patches(&scores, rows, cols, tile_w, tile_h, DILATION_RADIUS);
        patches.sort_by(|a, b| b.mean_score.partial_cmp(&a.mean_score).unwrap());

        println!(
            "\nDetected {} patch(es) (≥{MIN_PATCH_TILES} contiguous hot tiles):",
            patches.len()
        );
        for (i, p) in patches.iter().enumerate() {
            let (x, y, w, h) = p.bbox;
            println!(
                "  patch {}: {}×{} px at ({},{})  tiles={}  mean_score={:.2}",
                i, w, h, x, y, p.tile_count, p.mean_score
            );
        }

        // Overlay hot tiles with semi-transparent orange
        for row in 0..rows {
            for col in 0..cols {
                let s = scores[row][col];
                if s < HOT_THRESHOLD {
                    continue;
                }
                let alpha = (s / 20.0).clamp(0.15, 0.45);
                fill_rect_blend(
                    &mut annotated,
                    col as u32 * tile_w,
                    row as u32 * tile_h,
                    tile_w,
                    tile_h,
                    [255, 140, 0],
                    alpha,
                );
            }
        }
    }

    // Patch bounding boxes — cycle through distinct colors
    let patch_colors: &[Rgba<u8>] = &[
        Rgba([0, 220, 0, 255]),
        Rgba([0, 160, 255, 255]),
        Rgba([255, 60, 60, 255]),
        Rgba([220, 0, 220, 255]),
    ];
    let selected_count = patches.len().min(MAX_REGIONS);
    for (i, patch) in patches.iter().take(selected_count).enumerate() {
        let (x, y, w, h) = patch.bbox;
        draw_rect_border(
            &mut annotated,
            x,
            y,
            w,
            h,
            patch_colors[i % patch_colors.len()],
            4,
        );
    }

    let out_path = path.trim_end_matches(".png").to_string() + "_annotated.png";
    annotated.save(&out_path).expect("Failed to save annotated image");
    println!("\nAnnotated image: {}", out_path);

    if run_nsfw {
        run_nsfw_on_patches(&img, &patches[..selected_count]);
    } else if selected_count == 0 {
        println!("No patches detected — full-image NSFW score would be used as fallback.");
    }
}

#[cfg(feature = "nsfw")]
fn run_nsfw_on_patches(img: &DynamicImage, patches: &[Patch]) {
    use virtue_core::nsfw::NsfwClassifier;
    let classifier = match NsfwClassifier::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load NSFW model: {e}");
            return;
        }
    };
    let (w, h) = img.dimensions();
    println!("\nNSFW scores per patch:");
    let mut max_score = 0.0f32;
    for (i, patch) in patches.iter().enumerate() {
        let (x, y, pw, ph) = patch.bbox;
        let crop_w = pw.min(w.saturating_sub(x));
        let crop_h = ph.min(h.saturating_sub(y));
        let crop = img.crop_imm(x, y, crop_w, crop_h);
        match classifier.score(&crop) {
            Some(nsfw) => {
                println!(
                    "  patch {i}: {}×{} px  nsfw={:.3}  (mean_tile_score={:.2})",
                    crop_w, crop_h, nsfw, patch.mean_score
                );
                max_score = max_score.max(nsfw);
            }
            None => println!("  patch {i}: nsfw=error"),
        }
    }
    println!("Max NSFW score: {:.3}", max_score);
}

#[cfg(not(feature = "nsfw"))]
fn run_nsfw_on_patches(_img: &DynamicImage, _patches: &[Patch]) {
    eprintln!("--nsfw requires building with --features nsfw");
}
