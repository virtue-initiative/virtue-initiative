//! Finds the embedded images in screenshots and, with `--classify`, scores each one with the
//! NSFW model the way `RiskClassifier::classify_regions` does.
//!
//! ```sh
//! cargo run --release -p virtue-core --example extract_images -- [--classify] [--out DIR] SHOT.png...
//! ```
//!
//! For every input it writes `DIR/<name>_proof.png`: the screenshot with a colored box around
//! each region on the left, and the extracted crops with matching borders on the right. With
//! `--classify`, a bar under each crop shows its final score (red) out of 1.0. Each crop is
//! also saved as `DIR/<name>_<n>.png`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use image::{DynamicImage, GenericImageView, Rgba, RgbaImage, imageops};
use virtue_core::module::screenshot::image_regions::{Region, extract_regions};
use virtue_core::module::screenshot::risk_classifier::{MAX_REGIONS, RiskClassifier};

const MODEL_BYTES: &[u8] = include_bytes!("../models/nsfw_small_v1.nnef.tar");

const COLORS: [[u8; 3]; MAX_REGIONS] = [
    [230, 25, 75],
    [60, 180, 75],
    [0, 130, 200],
    [245, 130, 48],
    [145, 30, 180],
    [240, 50, 230],
];
const CLASSES: [&str; 5] = ["drawings", "hentai", "neutral", "porn", "sexy"];

fn main() {
    let mut classify = false;
    let mut out = PathBuf::from("extracted");
    let mut inputs = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--classify" => classify = true,
            "--out" => out = args.next().expect("--out needs a directory").into(),
            _ => inputs.push(PathBuf::from(arg)),
        }
    }
    if inputs.is_empty() {
        eprintln!("usage: extract_images [--classify] [--out DIR] SHOT.png...");
        std::process::exit(2);
    }
    std::fs::create_dir_all(&out).expect("create output directory");
    let classifier = RiskClassifier::new(MODEL_BYTES);

    for input in &inputs {
        let name = input.file_stem().unwrap().to_string_lossy();
        let img = image::open(input).expect("decode screenshot");
        let (w, h) = img.dimensions();

        // Warm-up run, then time the median of a few so the number reflects steady state.
        extract_regions(&img, MAX_REGIONS);
        let mut times: Vec<f64> = (0..5)
            .map(|_| {
                let start = Instant::now();
                extract_regions(&img, MAX_REGIONS);
                start.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        times.sort_by(f64::total_cmp);
        let regions = extract_regions(&img, MAX_REGIONS);
        println!(
            "{name} ({w}x{h}): {} region(s) in {:.1} ms",
            regions.len(),
            times[times.len() / 2]
        );

        let mut scores = vec![None; regions.len()];
        if classify {
            let start = Instant::now();
            let whole = classifier.classify_image(&img).expect("classify");
            println!(
                "  whole frame (current): {:.0} ms, skin {:.2}, nsfw {}, risk {:.2}",
                start.elapsed().as_secs_f64() * 1000.0,
                whole.skin,
                whole.nsfw.map_or("-".into(), |n| format!("{n:.2}")),
                whole.risk,
            );
            let start = Instant::now();
            let result = classifier.classify_regions(&img).expect("classify");
            println!(
                "  per region:            {:.0} ms, {} model run(s), skin {:.2}, nsfw {}, risk {:.2}",
                start.elapsed().as_secs_f64() * 1000.0,
                result.model_runs,
                result.scores.skin,
                result.scores.nsfw.map_or("-".into(), |n| format!("{n:.2}")),
                result.scores.risk,
            );
            for (i, r) in result.regions.iter().enumerate() {
                scores[i] = r.score;
                let probs = r.probs.map_or("skipped (skin gate)".into(), |p| {
                    (0..5)
                        .map(|c| format!("{} {:.2}", CLASSES[c], p[c]))
                        .collect::<Vec<_>>()
                        .join(", ")
                });
                let confirm = r.confirm_probs.map_or(String::new(), |p| {
                    format!(" | mirrored porn {:.2} sexy {:.2}", p[3], p[4])
                });
                println!(
                    "    #{} skin {:.2} score {} | {probs}{confirm}",
                    i + 1,
                    r.skin,
                    r.score.map_or("-".into(), |s| format!("{s:.2}")),
                );
            }
        }

        for (i, r) in regions.iter().enumerate() {
            println!(
                "  #{} {}x{} at ({}, {})",
                i + 1,
                r.width,
                r.height,
                r.x,
                r.y
            );
            img.crop_imm(r.x, r.y, r.width, r.height)
                .save(out.join(format!("{name}_{}.png", i + 1)))
                .expect("write crop");
        }
        let proof = proof_image(&img, &regions, &scores);
        let path = out.join(format!("{name}_proof.png"));
        proof.save(&path).expect("write proof");
        println!("  -> {}", display(&path));
    }
}

/// Screenshot with boxes on the left, crops in a 2×3 grid on the right.
fn proof_image(img: &DynamicImage, regions: &[Region], scores: &[Option<f32>]) -> RgbaImage {
    let (w, h) = img.dimensions();
    let panel_w = w * 2 / 3;
    let gap = 24;
    let mut canvas = RgbaImage::from_pixel(w + gap + panel_w, h, Rgba([32, 32, 36, 255]));
    imageops::overlay(&mut canvas, &img.to_rgba8(), 0, 0);

    let thickness = (w / 400).max(3);
    for (i, r) in regions.iter().enumerate() {
        stroke(
            &mut canvas,
            r.x,
            r.y,
            r.width,
            r.height,
            thickness,
            COLORS[i],
        );
    }

    let cols = 2;
    let rows = MAX_REGIONS.div_ceil(cols) as u32;
    let cell_w = panel_w / cols as u32;
    let cell_h = h / rows;
    let pad = 16;
    let bar_h = 10;
    for (i, r) in regions.iter().enumerate() {
        let cx = w + gap + (i % cols) as u32 * cell_w;
        let cy = (i / cols) as u32 * cell_h;
        let max_w = cell_w - 2 * pad;
        let max_h = cell_h - 3 * pad - bar_h;
        let scale = (max_w as f32 / r.width as f32).min(max_h as f32 / r.height as f32);
        let tw = ((r.width as f32 * scale) as u32).max(1);
        let th = ((r.height as f32 * scale) as u32).max(1);
        let crop = img
            .crop_imm(r.x, r.y, r.width, r.height)
            .resize_exact(tw, th, imageops::FilterType::Triangle)
            .to_rgba8();
        let x = cx + pad + (max_w - tw) / 2;
        let y = cy + pad;
        imageops::overlay(&mut canvas, &crop, x as i64, y as i64);
        stroke(&mut canvas, x - 4, y - 4, tw + 8, th + 8, 4, COLORS[i]);

        if let Some(score) = scores.get(i).copied().flatten() {
            let by = y + th + pad / 2;
            fill(&mut canvas, x, by, tw, bar_h, [80, 80, 86]);
            fill(
                &mut canvas,
                x,
                by,
                (tw as f32 * score) as u32,
                bar_h,
                [235, 64, 52],
            );
        }
    }
    canvas
}

fn stroke(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, t: u32, c: [u8; 3]) {
    fill(img, x, y, w, t, c);
    fill(img, x, (y + h).saturating_sub(t), w, t, c);
    fill(img, x, y, t, h, c);
    fill(img, (x + w).saturating_sub(t), y, t, h, c);
}

fn fill(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, c: [u8; 3]) {
    for py in y..(y + h).min(img.height()) {
        for px in x..(x + w).min(img.width()) {
            img.put_pixel(px, py, Rgba([c[0], c[1], c[2], 255]));
        }
    }
}

fn display(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}
