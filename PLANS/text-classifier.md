# NSFW Text Detection on Screenshots

> **Status: In Progress** — Architecture decided, implementation not yet started.

## Context

The Virtue Initiative is an accountability monitoring app. Clients capture screenshots and upload them (encrypted) to a Cloudflare Workers API. Screenshots go through an image pipeline before upload: Gaussian blur (σ=2.0) → resize to 128px minimum dimension → WebP encode. After this pipeline, images are too small and blurry for OCR. Therefore, text analysis **must happen before** the image pipeline runs — in `service.rs`, immediately after `platform.take_screenshot()` returns the original full-resolution image.

There is no existing AI/ML integration, no OCR, and no content moderation in the codebase. The `LogEntry.risk: Option<f32>` field (0.0–1.0) is already wired through the whole pipeline (storage, notifications, display), currently only used for device lifecycle events. We can populate it with content-based risk using the same field.

Image NSFW detection (nudity in pixels) is being handled separately. This task covers **text NSFW only** — inappropriate written content visible on screen (e.g. a spicy story, explicit chat messages).

## Recommended Approach

**Fully on-device pipeline**: OCR → local ONNX model inference. No data ever leaves the device for content analysis.

1. **OCR** via `leptess` (Rust bindings to Tesseract) — extracts text from the raw screenshot before blur/resize
2. **Text classification** via `ort` (ONNX Runtime for Rust) running a pre-trained NSFW/toxicity text classification model (DistilBERT-based, ~100–300MB)
3. The resulting `f32` risk score is stored in `LogEntry.risk`, which already triggers partner notifications at the high band (≥ 0.7)

Rejected alternatives:
- **External moderation API (OpenAI, Perspective API)**: Sends text off-device — violates on-device requirement
- **Vision LLM on original screenshot**: Sends full high-res pixels off-device, expensive, slow
- **Keyword/regex list**: Too easy to evade, high false-negative rate for sophisticated content
- **Rule-based classifier**: Not robust enough for "spicy story" detection

## Implementation Plan

### Step 1 — Add config fields (`client/core/src/config.rs`)

Add an optional model path field to `Config` and `RuntimeConfigFile`:

```rust
// In Config struct
pub text_classifier_model_path: Option<PathBuf>,  // path to ONNX model + tokenizer

// In RuntimeConfigFile
text_classifier_model_path: Option<String>,
```

Wire through `Config::new` and `refresh_from_runtime_file`. If `None`, the classifier is disabled and `analyze` returns `None`.

### Step 2 — New module: `client/core/src/content_analyzer.rs`

```rust
pub struct ContentAnalyzer { /* holds leptess instance, ort session, tokenizer */ }

impl ContentAnalyzer {
    // Returns None if: disabled (no model path), tessdata missing, model load fails
    pub fn new(config: &Config) -> Self { ... }

    // Returns None if: OCR finds < ~10 words, inference fails for any reason
    pub fn analyze(&self, screenshot: &Screenshot) -> Option<f32> {
        // 1. OCR via leptess on screenshot.bytes (original full-res, pre-pipeline)
        // 2. Skip if < ~10 words extracted
        // 3. Tokenize text
        // 4. Run ONNX model inference → logit for NSFW class
        // 5. Apply sigmoid → probability in [0, 1]
        // Any error → log warning, return None
    }
}
```

No HTTP client needed — entirely local.

### Step 3 — Wire into `MonitorService` (`client/core/src/service.rs`)

Add `content_analyzer: ContentAnalyzer` field, initialized in `setup`.

In `loop_iteration` (line 123–128), change:

```rust
// Before:
let screenshot = self.platform.take_screenshot()?;
let processed = self.process_screenshot(screenshot)?;

// After:
let screenshot = self.platform.take_screenshot()?;
let text_risk = self.content_analyzer.analyze(&screenshot);
let processed = self.process_screenshot(screenshot, text_risk)?;
```

Change `process_screenshot` to accept `Option<f32>` and forward it through to `prepare_screenshot_event`. Note: `prepare_screenshot_event` in `crypto.rs:116` currently hardcodes `risk: None` — update it to accept and pass the risk value.

Do the same for `capture_batch_screenshot` / `process_screenshot_with_data`: combine incoming `risk` with `text_risk` using `f32::max` (image NSFW from the parallel task will also flow through here eventually).

### Step 4 — Add dependencies to `client/core/Cargo.toml`

```toml
leptess = { version = "0.14", optional = true }
ort = { version = "2", optional = true }
tokenizers = { version = "0.20", optional = true }

[features]
content-analysis = ["leptess", "ort", "tokenizers"]
```

Default-enable for desktop targets. Mobile (Android/iOS) can be deferred — `ContentAnalyzer::analyze` returns `None` when the feature is absent.

**Packaging notes**:
- `leptess` links to system `libtesseract`. Add to `.deb` depends on Linux; document Homebrew install for macOS; use static build for Windows.
- `ort` links to ONNX Runtime native library (ships as a pre-built `.so`/`.dll`/`.dylib`).
- Tessdata (`eng.traineddata`, ~4MB) must be present at a known path; gracefully disable if missing.

### Step 5 — Model selection and bundling (TBD)

The specific model is not yet chosen. Requirements:
- DistilBERT-based (or smaller) for reasonable inference latency on end-user CPUs
- Fine-tuned for NSFW/explicit text detection — must handle "spicy story" style content, not just toxic comments
- Exported to ONNX format
- HuggingFace `tokenizers`-compatible tokenizer config

Candidates to evaluate:
- `unitary/toxic-bert` (BERT-base, ~440MB)
- A DistilBERT fine-tuned on NSFW text datasets (~250MB)
- A custom fine-tune on appropriate data if existing models have poor recall for story-style content

The model `.onnx` file and tokenizer JSON will be bundled with the app distribution (or downloaded on first run to the `state_dir`).

### Step 6 — Platform config wiring (Android / iOS, deferred)

- `client/android/rust/src/lib.rs`: accept and persist `text_classifier_model_path` in `write_runtime_overrides`
- `client/android/app/.../OverrideSettings.kt`: add optional field
- iOS: pass through via existing C FFI config mechanism

## Critical Files

| File | Change |
|------|--------|
| `client/core/src/config.rs` | Add `text_classifier_model_path` to `Config` and `RuntimeConfigFile` |
| `client/core/src/content_analyzer.rs` | **New file** — `ContentAnalyzer` with OCR + local ONNX inference |
| `client/core/src/service.rs` | Add `content_analyzer` field; call `analyze` before `process_screenshot`; update signature |
| `client/core/src/crypto.rs` | Pass risk through in `prepare_screenshot_event` (currently hardcoded `None`) |
| `client/core/Cargo.toml` | Add `leptess`, `ort`, `tokenizers` optional deps + `content-analysis` feature |
| `client/android/rust/src/lib.rs` | Wire new config field (deferred) |
| `client/android/app/.../OverrideSettings.kt` | Add model path field (deferred) |

## Key Design Decisions

- **Fully on-device**: No text, pixels, or extracted content leaves the device. Only the `f32` risk score is sent to the Virtue server.
- **Failure-safe**: Any error in OCR or inference returns `None` — screenshot capture is never blocked.
- **Risk combination**: `f32::max(text_risk, image_risk)` when the image NSFW task also produces scores. Both feed the same `LogEntry.risk` field.
- **Short text gate**: Skip inference if OCR extracts fewer than ~10 words to avoid cost/noise on mostly-visual screens.
- **Optional feature**: The `content-analysis` Cargo feature allows mobile builds to skip the heavy dependencies until cross-compilation is worked out.

## Open Questions

- [ ] Which specific ONNX model to use? Need to evaluate recall on "spicy story" style content vs toxic comment datasets.
- [ ] Model distribution: bundled in binary vs downloaded to `state_dir` on first run?
- [ ] Inference latency budget: what is acceptable added time per screenshot interval?
- [ ] Mobile support timeline: prioritize desktop-only first?

## Verification

1. `cargo build -p virtue-core --features content-analysis` — confirms dependencies compile
2. Unit test: `ContentAnalyzer::analyze` with a fixture screenshot of explicit text → assert score ≥ 0.7
3. Unit test: fixture screenshot of clean UI text → assert score < 0.4 or `None`
4. End-to-end: run Linux client with model path configured, screenshot a visible text document, observe risk in web dashboard
5. Verify `notifyPartnersAboutRiskLog` fires for risk ≥ 0.7 content score
