//! Desert Ant Labs' Moderator model (https://huggingface.co/desert-ant-labs/moderator), run
//! through tract from an NNEF conversion of its `moderator.tflite`.
//!
//! The crops and resampling mirror the SDK's `Sources/Moderator/Preprocess.swift`
//! (github.com/Desert-Ant-Labs/desert-ant-core): a letterboxed full image plus center squares
//! of 85%, 70% and 55% of the short side, each resized to 384×384 with Pillow's fixed-point
//! bilinear filter. The model normalizes internally, so pixels go in as raw 0..255 floats.
//! Its 5 outputs are independent sigmoid heads: nipples, genitals, buttocks, nude, sexAct.

use image::{DynamicImage, RgbImage};
use tract_nnef::prelude::*;

pub const SIDE: usize = 384;
pub const HEADS: [&str; 5] = ["nipples", "genitals", "buttocks", "nude", "sexAct"];
const ZOOMS: [(&str, f64); 3] = [("z85", 0.85), ("z70", 0.70), ("z55", 0.55)];

type Plan = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

pub struct Moderator(Plan);

impl Moderator {
    pub fn load(nnef_tar: &[u8]) -> TractResult<Self> {
        let plan = tract_nnef::nnef()
            .with_tract_core()
            .model_for_read(&mut std::io::Cursor::new(nnef_tar))?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self(plan))
    }

    fn run(&self, rgb: &[u8]) -> TractResult<[f32; 5]> {
        let input: Tensor =
            tract_ndarray::Array4::from_shape_fn((1, SIDE, SIDE, 3), |(_, y, x, c)| {
                rgb[(y * SIDE + x) * 3 + c] as f32
            })
            .into();
        let out = self.0.run(tvec![input.into()])?;
        let view = out[0].to_array_view::<f32>()?;
        Ok(std::array::from_fn(|i| view.as_slice().unwrap()[i]))
    }

    /// Head scores for every crop the SDK's quality modes use, by name: `fast` (center square
    /// of the short side), `letterbox`, `z85`, `z70`, `z55`, and `*_m` mirrors of the last four.
    /// fast = [fast]; balanced = [letterbox, z85, z70, z55]; accurate = balanced + mirrors.
    pub fn crops(&self, img: &DynamicImage) -> TractResult<Vec<(String, [f32; 5])>> {
        let rgb = img.to_rgb8();
        let (w, h) = (rgb.width() as i64, rgb.height() as i64);
        let short = w.min(h);
        let long = w.max(h);
        let square = |s: i64| ((w - s) / 2, (h - s) / 2, s);
        let mut windows = vec![
            ("fast".to_string(), square(short)),
            (
                "letterbox".to_string(),
                (-((long - w) / 2), -((long - h) / 2), long),
            ),
        ];
        for (name, zoom) in ZOOMS {
            windows.push((name.to_string(), square((short as f64 * zoom) as i64)));
        }
        let mut out = Vec::new();
        for (name, (x0, y0, size)) in windows {
            let crop = resample(&rgb, x0, y0, size as usize);
            out.push((name.clone(), self.run(&crop)?));
            if name != "fast" {
                out.push((format!("{name}_m"), self.run(&mirrored(&crop))?));
            }
        }
        Ok(out)
    }
}

fn mirrored(rgb: &[u8]) -> Vec<u8> {
    let mut out = rgb.to_vec();
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (src, dst) = ((y * SIDE + SIDE - 1 - x) * 3, (y * SIDE + x) * 3);
            out[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
        }
    }
    out
}

// Pillow's bilinear resampler (libImaging/Resample.c), as ported in the SDK.
const PRECISION_BITS: i32 = 32 - 8 - 2;

struct Taps {
    start: Vec<usize>,
    count: Vec<usize>,
    weights: Vec<i32>,
    ksize: usize,
}

fn taps(in_size: usize, out_size: usize) -> Taps {
    let scale = in_size as f64 / out_size as f64;
    let filter_scale = scale.max(1.0);
    let support = filter_scale;
    let ss = 1.0 / filter_scale;
    let ksize = support.ceil() as usize * 2 + 1;
    let mut t = Taps {
        start: Vec::new(),
        count: Vec::new(),
        weights: vec![0; out_size * ksize],
        ksize,
    };
    let mut k = vec![0f64; ksize];
    for xx in 0..out_size {
        let center = (xx as f64 + 0.5) * scale;
        let xmin = ((center - support + 0.5) as i64).max(0) as usize;
        let xmax = ((center + support + 0.5) as i64).min(in_size as i64) as usize - xmin;
        let mut total = 0.0;
        for (x, kx) in k.iter_mut().enumerate().take(xmax) {
            let d = (((x + xmin) as f64 - center + 0.5) * ss).abs();
            *kx = if d < 1.0 { 1.0 - d } else { 0.0 };
            total += *kx;
        }
        let row = &mut t.weights[xx * ksize..];
        for (w, &kx) in row.iter_mut().zip(&k).take(xmax) {
            let v = if total != 0.0 { kx / total } else { kx };
            let fixed = v * (1i64 << PRECISION_BITS) as f64;
            *w = if v < 0.0 { fixed - 0.5 } else { fixed + 0.5 } as i32;
        }
        t.start.push(xmin);
        t.count.push(xmax);
    }
    t
}

fn clip8(v: i32) -> u8 {
    (v >> PRECISION_BITS).clamp(0, 255) as u8
}

/// The `size`×`size` window at (`x0`, `y0`) resized to SIDE×SIDE; outside the image is black.
fn resample(img: &RgbImage, x0: i64, y0: i64, size: usize) -> Vec<u8> {
    let hz = taps(size, SIDE);
    let vt = taps(size, SIDE);
    let row_first = vt.start[0];
    let rows = vt.start[SIDE - 1] + vt.count[SIDE - 1] - row_first;
    let half = 1i32 << (PRECISION_BITS - 1);
    let (w, h) = (img.width() as i64, img.height() as i64);
    let src = img.as_raw();

    let mut temp = vec![0u8; rows * SIDE * 3];
    for r in 0..rows {
        let sy = y0 + (row_first + r) as i64;
        if sy < 0 || sy >= h {
            continue;
        }
        let row_base = (sy * w * 3) as usize;
        for xx in 0..SIDE {
            let mut s = [half; 3];
            for x in 0..hz.count[xx] {
                let sx = x0 + (hz.start[xx] + x) as i64;
                if sx < 0 || sx >= w {
                    continue;
                }
                let p = row_base + sx as usize * 3;
                let weight = hz.weights[xx * hz.ksize + x];
                for c in 0..3 {
                    s[c] = s[c].wrapping_add((src[p + c] as i32).wrapping_mul(weight));
                }
            }
            let o = (r * SIDE + xx) * 3;
            for c in 0..3 {
                temp[o + c] = clip8(s[c]);
            }
        }
    }

    let mut out = vec![0u8; SIDE * SIDE * 3];
    for yy in 0..SIDE {
        let first = vt.start[yy] - row_first;
        for xx in 0..SIDE {
            let mut s = [half; 3];
            for y in 0..vt.count[yy] {
                let p = ((first + y) * SIDE + xx) * 3;
                let weight = vt.weights[yy * vt.ksize + y];
                for c in 0..3 {
                    s[c] = s[c].wrapping_add((temp[p + c] as i32).wrapping_mul(weight));
                }
            }
            let o = (yy * SIDE + xx) * 3;
            for c in 0..3 {
                out[o + c] = clip8(s[c]);
            }
        }
    }
    out
}
