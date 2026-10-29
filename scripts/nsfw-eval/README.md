# NSFW classifier evaluation

Tools for measuring and tuning the screenshot NSFW scoring in
`client/core/src/module/screenshot/risk_classifier.rs` against labeled screenshots, without
keeping unblurred screenshots on disk.

## 1. Collect

```bash
scripts/nsfw-eval/collect.sh ARCHIVE.tar.gz.enc PASSPHRASE_FILE OUT_DIR [collector options]
```

The archive is an `openssl enc -aes-256-cbc -pbkdf2 -iter 600000` encrypted `.tar.gz` of PNG
screenshots with labels: a `labels.jsonl` (each file's `nsfw` flag, `category`, `kind` and
`size`) and/or a `manifest.json` (a `target` per file; `normal-*` is safe, anything else is
NSFW). PNGs at the archive root are collected into `OUT_DIR`; otherwise each subdirectory
holding PNGs (`desktop/`, `mobile-android/`, ...) goes to `OUT_DIR/<subdirectory>`. The
script decrypts into `/dev/shm`, runs the `nsfw_eval` example
(`client/core/examples/nsfw_eval/`) and deletes the plaintext. Each output directory gets:

- `meta.json`: the model, the extraction parameters and the Rust scoring constants used.
- `shots.jsonl`: per screenshot, raw model outputs for the whole frame and for every
  extracted region (up to 12), each also mirrored, plus skin and grayscale scores and the
  window each region was found in. Nothing is gated, so any gate or threshold can be
  replayed offline. With default extraction settings it also stores what Rust's
  `classify_regions` returned, for the parity check below.
- `blur/`: each screenshot and region as the blurred, compressed WebP the upload pipeline
  produces.
- `labels.jsonl` / `manifest.json`: the labels.
- `bench.json`: model build time, time per run and memory, which `score.py` uses to
  estimate the CPU cost of each screenshot.

Collector options are listed at the top of `client/core/examples/nsfw_eval/main.rs`. The
useful ones:

- Extraction thresholds (`--gutter-slack`, `--min-photo-fill`, `--min-edge-run`, ...): any
  change needs a new collect run. Write it to a new `OUT_DIR` to compare.
- `--grayscale` turns every screenshot gray first, the way an OS grayscale filter would.
- `--screen desktop|phone` overrides how screenshots are treated; by default a portrait
  screenshot is a phone and a landscape one a desktop.
- `--model none` skips the model, for quick extraction-only runs.
- `--model moderator:PATH.nnef.tar` runs Desert Ant Labs' Moderator
  (huggingface.co/desert-ant-labs/moderator) instead of the shipped `nsfw_small_v1`, from
  an NNEF conversion of its `.tflite` with the SDK's own crops and resampling
  (`moderator.rs`). Stock tract can't read that `.tflite`, so the conversion uses a patched
  tract-tflite; the converter and patch live outside the repo, next to the converted model.

## 2. Score, sweep, report

```bash
scripts/nsfw-eval/score.py OUT_DIR [OUT_DIR...]
scripts/nsfw-eval/score.py OUT_DIR... --set region_score_floor=0.5 --set min_agree=2
scripts/nsfw-eval/score.py OUT_DIR... --sweep agree_at=0.8,0.9 --sweep max_regions=6,12
scripts/nsfw-eval/score.py OUT_DIR... --csv shots.csv --json scored.json --report report.html
```

`score.py` (Python 3, standard library only) replays both today's whole-frame scoring
(`classify_image`) and `classify_regions` over the stored outputs. Pass result directories
from several models to compare them side by side. Its defaults come from `meta.json` and
match Rust, so with no `--set` it must reproduce `classify_regions` exactly; it checks that
on every run and prints any mismatch. If you change the scoring in Rust, change
`score_regions` in `score.py` to match. Run `score.py --help` for every parameter.

`--report` writes a self-contained HTML file with the blurred thumbnails inlined. It holds
screenshots of whatever was in the archive, so keep it local. `--artifact` writes the same
page with no images (region outlines on blank frames), which is safe to share. `--variant`
adds summary rows with other settings to either page.
