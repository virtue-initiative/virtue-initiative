# NSFW Detection — Implementation Summary

## Goal

Attach a local NSFW score to every screenshot `BatchEvent` so the existing
`risk` field flows through to the web filtering UI and the partner-alert email
trigger (`risk >= 0.4` in `api/src/lib/tamper.ts`).

## What was built

### New module — `client/core/src/nsfw.rs`

Owns the classifier behind a stable public API:

```rust
pub struct NsfwClassifier { … }
impl NsfwClassifier {
    pub fn new() -> CoreResult<Self>;
    pub fn score(&self, decoded: &image::DynamicImage) -> Option<f32>;
}
```

- **v1 backend**: the [`nsfw` crate](https://crates.io/crates/nsfw) (0.2.0),
  a pure-Rust MobileNet ONNX classifier backed by `tract-onnx`.
- **Score combiner**: `max(porn, hentai, sexy)` clamped to `[0.0, 1.0]`.
- **Error handling**: any classifier error is swallowed to `None` (logged via
  `eprintln!`) so a flaky model never blocks screenshot capture.
- **Feature-gated**: the real impl lives under `#[cfg(feature = "nsfw")]`;
  a zero-dep stub (`new()` → `Err`, `score()` → `None`) compiles when the
  feature is off.

### Model delivery — `build.rs` + auto-download

The ONNX model (~17 MB) is **not committed** to the repo. `build.rs` handles it:

1. If `NSFW_MODEL_PATH` env var is set, use that file (CI cache / air-gapped builds).
2. Otherwise, look for `$OUT_DIR/nsfw_model.onnx`. If present, reuse it.
3. If absent, download from
   `https://github.com/Fyko/nsfw/releases/download/v0.2.0/model.onnx`
   via `curl`.

The resolved path is exported as `cargo:rustc-env=VIRTUE_NSFW_MODEL_PATH` and
consumed by `include_bytes!(env!("VIRTUE_NSFW_MODEL_PATH"))` in `nsfw.rs`, so
the model bytes are embedded in the binary at compile time — no runtime download.

`client/core/assets/nsfw_model.onnx` is gitignored.
`client/core/assets/test_fixture.png` (4×4 grey PNG used by tests) is committed.
Model provenance is documented in `client/core/assets/README.md`.

### Image pipeline — `client/core/src/image_pipeline.rs`

Added `ImagePipeline::process_decoded(decoded: DynamicImage, captured_at_ms)`
so the raw frame can be scored before blurring/resizing without decoding twice.
The existing `process()` delegates to it.

### Wired into capture — `client/core/src/service.rs`

- `MonitorService` gained `classifier: Option<NsfwClassifier>`.
- `setup()` calls `NsfwClassifier::new()`; on failure logs to `errors.log`
  and stores `None` — classifier failure never aborts startup.
- Both `process_screenshot` and `process_screenshot_with_data` now:
  1. Decode the raw bytes once.
  2. Score the decoded image via the classifier.
  3. Pass the result through `process_decoded` (blur → resize → WebP).
  4. Attach the score to the `BatchEvent` via `prepare_screenshot_batch_event`.
- For `capture_batch_screenshot` callers that supply an explicit `risk`, the
  caller wins: `caller_risk.or(classifier_score)`.

### Cargo feature — `client/core/Cargo.toml`

```toml
[features]
default = ["nsfw"]
nsfw = ["dep:nsfw"]

[dependencies]
nsfw = { version = "0.2.0", optional = true }
```

Disable with `--no-default-features` if binary size or cross-compilation is a
problem (e.g. mobile targets).

### Docs — `client/core/architecture.md`

The Screenshot Model section now documents the classifier step and the
`NsfwClassifier` module.

## Tests added

| Location | Test | What it checks |
|---|---|---|
| `nsfw.rs` | `benign_image_scores_below_threshold` | Solid-colour image → score < 0.4 |
| `nsfw.rs` | `score_is_always_in_range` | Score in `[0.0, 1.0]` |
| `nsfw.rs` | `malformed_bytes_returns_none` | `score()` never panics |
| `service.rs` | `process_screenshot_populates_risk_field` | No `Err` from the full pipeline |
| `service.rs` | `capture_batch_screenshot_explicit_risk_wins` | Caller's `Some(0.9)` is preserved |

All tests pass with and without the `nsfw` feature.

## Known follow-ups (out of scope)

- **Score calibration**: the raw model output may need a floor or non-linear
  remap before the `>= 0.4` alert threshold is reliable on real traffic.
- **Downloadable model swap**: `NsfwClassifier`'s module boundary is designed
  to absorb a runtime-downloadable or larger replacement model without touching
  `service.rs`.
- **Mobile binary size**: if the embedded model makes mobile targets too large,
  build those with `--no-default-features` until a lighter model is bundled.
- **CI caching**: set `NSFW_MODEL_PATH` in CI to a cached copy to avoid
  downloading the model on every fresh build.
