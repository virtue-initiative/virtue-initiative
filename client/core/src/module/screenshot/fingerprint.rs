//! Screen-change fingerprint used to dedup redundant screenshot uploads.
//!
//! A captured frame is downscaled to a small grayscale grid and two fingerprints are compared.
//! A frame counts as *changed* if EITHER:
//!   * the **mean** per-cell delta is high — a broad change, including low-contrast ones like a
//!     terminal filling with text (this is the case a whole-frame approach misses); or
//!   * a **number of cells** changed strongly — a concentrated change like a video window.
//!
//! The grid resolution is **derived from the image size** (each cell covers a roughly fixed
//! `TARGET_CELL_PX`-sized source block) rather than a fixed 32×32. On a large/wide display a
//! fixed grid makes each cell average thousands of source pixels, which dilutes real changes
//! (e.g. text on a dark terminal) below any threshold — the bug this addresses. Sizing the grid
//! to the image keeps per-cell dilution bounded and the metrics stable across resolutions.
//!
//! Two captures of the same screen produce identically-shaped grids and so are directly
//! comparable; if the resolution changes the shapes differ and `changed` fails safe to `true`.
//!
//! Thresholds were calibrated against a real "minimum change that should be detected" sample
//! (a terminal filling with text across one of two monitors on a 3840×1080 desktop). See the
//! unit tests for the calibration table.

use image::GenericImageView;
use serde::{Deserialize, Serialize};

use crate::error::CoreResult;

/// Approximate source pixels per grid cell on each axis. The grid is sized so a cell covers
/// roughly this much screen, independent of resolution.
const TARGET_CELL_PX: u32 = 32;
/// Clamp for the per-axis cell count, so tiny screens still get a usable grid and huge ones
/// don't produce an unbounded fingerprint.
const GRID_MIN: u32 = 16;
const GRID_MAX: u32 = 128;

/// Mean per-cell absolute delta (0..=255) at or above which the frame is "changed". Catches
/// broad changes, including low-contrast ones a single-cell rule would miss. Naturally
/// size-normalized (it's an average), so it holds across grid resolutions.
const MEAN_DELTA_THRESHOLD: f64 = 0.5;

/// Per-cell absolute delta (0..=255) at or above which a cell is counted as "strongly changed".
const STRONG_CELL_DELTA: u8 = 40;
/// Number of strongly-changed cells at or above which the frame is "changed", regardless of the
/// mean. A clock/cursor touches 1–2 cells; a video window touches many, so the *count* (not a
/// single max) cleanly separates them and stays size-independent (cell source-size is fixed).
const MIN_STRONG_CELLS: usize = 6;

/// A grid fingerprint of a captured frame: the grid dimensions plus the row-major grayscale
/// cells. Dimensions are stored so two fingerprints can only be compared when they describe the
/// same-shaped grid (i.e. the same screen resolution).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub width: u16,
    pub height: u16,
    pub cells: Vec<u8>,
}

/// Hand-written so the raw grayscale grid — a coarse but real reconstruction
/// of on-screen content — never reaches a log line verbatim.
impl std::fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Fingerprint {{ width: {}, height: {}, cells: <{} bytes> }}",
            self.width,
            self.height,
            self.cells.len()
        )
    }
}

/// Grid cell counts (width, height) for an image of the given pixel dimensions.
pub(crate) fn grid_dims(width: u32, height: u32) -> (u32, u32) {
    let axis =
        |d: u32| ((d as f32 / TARGET_CELL_PX as f32).round() as u32).clamp(GRID_MIN, GRID_MAX);
    (axis(width), axis(height))
}

/// Reduce an encoded image to a size-appropriate grayscale grid fingerprint.
pub fn fingerprint(image_bytes: &[u8]) -> CoreResult<Fingerprint> {
    let decoded = image::load_from_memory(image_bytes)?;
    let (width, height) = decoded.dimensions();
    let (grid_w, grid_h) = grid_dims(width, height);
    let grid = decoded
        .resize_exact(grid_w, grid_h, image::imageops::FilterType::Triangle)
        .to_luma8();
    Ok(Fingerprint {
        width: grid_w as u16,
        height: grid_h as u16,
        cells: grid.into_raw(),
    })
}

/// True if `cur` differs materially from `prev`.
///
/// Fails safe to `true` (treat as changed → upload) when the two fingerprints aren't directly
/// comparable (different grid shape, e.g. a resolution change), so a mismatch can never silently
/// suppress an upload.
pub fn changed(prev: &Fingerprint, cur: &Fingerprint) -> bool {
    if prev.width != cur.width
        || prev.height != cur.height
        || prev.cells.len() != cur.cells.len()
        || cur.cells.is_empty()
    {
        return true;
    }

    let mut strong_cells = 0usize;
    let mut total_delta = 0u64;
    for (&a, &b) in prev.cells.iter().zip(cur.cells.iter()) {
        let delta = a.abs_diff(b);
        total_delta += delta as u64;
        if delta >= STRONG_CELL_DELTA {
            strong_cells += 1;
        }
    }

    let mean_delta = total_delta as f64 / cur.cells.len() as f64;
    mean_delta >= MEAN_DELTA_THRESHOLD || strong_cells >= MIN_STRONG_CELLS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(width: u16, height: u16, cells: Vec<u8>) -> Fingerprint {
        assert_eq!(cells.len(), width as usize * height as usize);
        Fingerprint {
            width,
            height,
            cells,
        }
    }

    #[test]
    fn grid_dims_scale_with_image_size() {
        // 3840×1080 dual-monitor desktop: ~32px cells, aspect preserved (not squashed to square).
        assert_eq!(grid_dims(3840, 1080), (120, 34));
        // 1080p single monitor.
        assert_eq!(grid_dims(1920, 1080), (60, 34));
        // Small images clamp up to GRID_MIN so the grid stays usable.
        assert_eq!(grid_dims(64, 64), (16, 16));
        // Very large displays clamp down to GRID_MAX.
        assert_eq!(grid_dims(8192, 4096), (128, 128));
    }

    #[test]
    fn identical_grids_are_unchanged() {
        let g = fp(60, 34, vec![120; 60 * 34]);
        assert!(!changed(&g, &g));
    }

    #[test]
    fn broad_low_contrast_change_is_detected() {
        // The regression case: a terminal fills with text across part of a wide screen. No single
        // cell moves dramatically, but a large area shifts a little — the mean rule must catch it.
        // (On the real 3840×1080 sample this measured mean ≈ 1.9; here we model ~40% of cells
        // nudging by 6, mean ≈ 2.4.)
        let prev = fp(120, 34, vec![100; 120 * 34]);
        let mut cur = prev.clone();
        for c in cur.cells.iter_mut().take((120 * 34) * 4 / 10) {
            *c += 6;
        }
        assert!(changed(&prev, &cur));
    }

    #[test]
    fn clock_sized_one_cell_jitter_is_unchanged() {
        // A clock/cursor moves one or two cells. Too few strong cells, negligible mean → unchanged.
        let prev = fp(120, 34, vec![100; 120 * 34]);
        let mut cur = prev.clone();
        cur.cells[500] = 180; // one cell, +80
        cur.cells[501] = 170;
        assert!(!changed(&prev, &cur));
    }

    #[test]
    fn a_few_strong_cells_are_unchanged() {
        // Below MIN_STRONG_CELLS and tiny mean: e.g. a small status indicator updating.
        let prev = fp(120, 34, vec![60; 120 * 34]);
        let mut cur = prev.clone();
        for i in 0..MIN_STRONG_CELLS - 1 {
            cur.cells[i] = 160; // +100, strongly changed
        }
        assert!(!changed(&prev, &cur));
    }

    #[test]
    fn concentrated_region_change_is_detected() {
        // A small video window: a cluster of strongly-changed cells whose *mean* over the whole
        // frame stays below threshold. The strong-cell-count rule (not the mean) must catch it —
        // this is what keeps a small corner video from being diluted away.
        let prev = fp(120, 34, vec![30; 120 * 34]);
        let mut cur = prev.clone();
        for i in 0..(MIN_STRONG_CELLS + 4) {
            cur.cells[i] = 130; // +100
        }
        let total: u64 = prev
            .cells
            .iter()
            .zip(&cur.cells)
            .map(|(&a, &b)| a.abs_diff(b) as u64)
            .sum();
        let mean = total as f64 / cur.cells.len() as f64;
        assert!(
            mean < MEAN_DELTA_THRESHOLD,
            "strong-cell rule, not mean, should fire"
        );
        assert!(changed(&prev, &cur));
    }

    #[test]
    fn different_grid_shape_fails_safe_to_changed() {
        // Resolution change → different grid shape → always upload.
        assert!(changed(
            &fp(60, 34, vec![10; 60 * 34]),
            &fp(120, 34, vec![10; 120 * 34])
        ));
    }

    #[test]
    fn fingerprint_dims_match_grid_dims_and_are_stable() {
        let png = crate::testing::fixtures::solid_png_bytes(120);
        let a = fingerprint(&png).expect("fingerprint solid png");
        let (gw, gh) = grid_dims(64, 64); // solid_png_bytes is 64×64
        assert_eq!((a.width as u32, a.height as u32), (gw, gh));
        assert_eq!(a.cells.len(), (gw * gh) as usize);
        let b = fingerprint(&png).expect("fingerprint again");
        assert!(!changed(&a, &b));
    }
}
