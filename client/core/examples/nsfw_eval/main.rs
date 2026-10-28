//! Collects raw NSFW classifier data for a directory of screenshots, for offline tuning with
//! `scripts/nsfw-eval/score.py`. Normally run through `scripts/nsfw-eval/collect.sh`.
//!
//! ```sh
//! cargo run --release -p virtue-core --example nsfw_eval -- --out DIR [options] SHOTS_DIR
//! cargo run --release -p virtue-core --example nsfw_eval -- --bench [--model M]
//! cargo run --release -p virtue-core --example nsfw_eval -- --probe [--model M] IMG...
//! ```
//!
//! Nothing is gated here: the model runs on the whole frame and on every extracted region, so
//! the scorer can replay any gate or threshold without the model. Writes:
//!
//! - `DIR/meta.json`: the model, the extraction parameters and the Rust scoring constants.
//! - `DIR/shots.jsonl`: one record per screenshot (see `record` below).
//! - `DIR/blur/*.webp`: each screenshot and each region, blurred and compressed by the same
//!   pipeline uploads go through. No unblurred pixels are written.
//!
//! `--model` picks what runs on each view (the whole frame or one region):
//!
//! - `gantman` (default): the shipped `nsfw_small_v1`; stores `probs` and `mirror_probs`.
//! - `moderator:PATH.nnef.tar`: Desert Ant Labs' Moderator converted to NNEF; stores the
//!   head scores of every SDK crop under `crops` (see `moderator.rs`).
//! - `none`: no model, just extraction, for quick runs while changing the extractor.
//!
//! `--bench` prints model load time, inference time and memory for the chosen model as JSON.
//! `--probe` prints the raw view output for each image given.
//!
//! `--grayscale` turns every screenshot gray before anything else sees it, the way an OS
//! grayscale filter would, to measure how detection holds up against one.
//!
//! Other options: `--max-regions N` (default 12), `--jobs N`, and the extraction thresholds
//! `--photo-colors`, `--muted-photo-colors`, `--max-dominant-share`, `--gutter-tolerance`,
//! `--gutter-slack`,
//! `--min-side`, `--min-photo-fill`, `--edge-contrast`, `--min-edge-run` (0 turns edge cuts
//! off) and `--edge-gap`. Each region records the window it was found in (`container`).
//!
//! `--screen desktop|phone` says what kind of screen the screenshots come from, for
//! `classify_regions`; by default a portrait screenshot is a phone and a landscape one a
//! desktop. Each record says which was used.

mod moderator;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use image::{DynamicImage, GenericImageView};
use serde_json::{Value, json};
use virtue_core::module::screenshot::image_pipeline::ImagePipeline;
use virtue_core::module::screenshot::image_regions::{ExtractParams, extract_regions_tagged};
use virtue_core::module::screenshot::risk_classifier::{
    LoadedModel, RiskClassifier, Screen, is_grayscale, scoring_constants, skin_score,
};

const MODEL_BYTES: &[u8] = include_bytes!("../../models/nsfw_small_v1.nnef.tar");
const GANTMAN_CLASSES: [&str; 5] = ["drawings", "hentai", "neutral", "porn", "sexy"];

#[derive(Clone)]
enum ModelSpec {
    None,
    Gantman,
    Moderator(PathBuf),
}

enum Model {
    None,
    Gantman(LoadedModel),
    Moderator(moderator::Moderator),
}

impl ModelSpec {
    fn parse(s: &str) -> Self {
        match s.split_once(':') {
            None if s == "gantman" => Self::Gantman,
            None if s == "none" => Self::None,
            Some(("moderator", path)) => Self::Moderator(PathBuf::from(path)),
            _ => panic!("unknown --model {s}; use gantman or moderator:PATH.nnef.tar"),
        }
    }

    fn meta(&self) -> Value {
        match self {
            Self::None => json!({ "id": "none", "name": "extraction only" }),
            Self::Gantman => json!({
                "id": "gantman", "name": "nsfw_small_v1", "kind": "softmax",
                "labels": GANTMAN_CLASSES, "file_bytes": MODEL_BYTES.len(),
            }),
            Self::Moderator(path) => json!({
                "id": "moderator", "name": "desert-ant-labs/moderator", "kind": "sigmoid_heads",
                "labels": moderator::HEADS, "file": path.display().to_string(),
                "file_bytes": fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            }),
        }
    }

    fn load(&self) -> Model {
        match self {
            Self::None => Model::None,
            Self::Gantman => Model::Gantman(
                RiskClassifier::new(MODEL_BYTES)
                    .load_model()
                    .expect("build model"),
            ),
            Self::Moderator(path) => Model::Moderator(
                moderator::Moderator::load(&fs::read(path).expect("read model"))
                    .expect("build model"),
            ),
        }
    }
}

impl Model {
    /// Raw outputs for one view and how many model runs they took.
    fn view(&self, img: &DynamicImage) -> (serde_json::Map<String, Value>, u32) {
        let mut out = serde_json::Map::new();
        match self {
            Self::None => (out, 0),
            Self::Gantman(model) => {
                out.insert("probs".into(), json!(model.probs(img).expect("inference")));
                let mirror = model.probs(&img.fliph()).expect("inference");
                out.insert("mirror_probs".into(), json!(mirror));
                (out, 2)
            }
            Self::Moderator(model) => {
                let crops = model.crops(img).expect("inference");
                let runs = crops.len() as u32;
                let map: serde_json::Map<_, _> =
                    crops.into_iter().map(|(k, v)| (k, json!(v))).collect();
                out.insert("crops".into(), Value::Object(map));
                (out, runs)
            }
        }
    }
}

struct Options {
    out: Option<PathBuf>,
    inputs: Vec<PathBuf>,
    max_regions: usize,
    jobs: usize,
    params: ExtractParams,
    model: ModelSpec,
    bench: bool,
    probe: bool,
    grayscale: bool,
    /// None: a portrait screenshot is a phone, a landscape one a desktop.
    screen: Option<Screen>,
}

fn parse_args() -> Options {
    let mut opts = Options {
        out: None,
        inputs: Vec::new(),
        max_regions: 12,
        jobs: std::thread::available_parallelism()
            .map_or(2, |n| n.get() / 2)
            .max(1),
        params: ExtractParams::default(),
        model: ModelSpec::Gantman,
        bench: false,
        probe: false,
        grayscale: false,
        screen: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{arg} needs a value"));
        let p = &mut opts.params;
        match arg.as_str() {
            "--out" => opts.out = Some(PathBuf::from(value())),
            "--model" => opts.model = ModelSpec::parse(&value()),
            "--bench" => opts.bench = true,
            "--probe" => opts.probe = true,
            "--grayscale" => opts.grayscale = true,
            "--max-regions" => opts.max_regions = value().parse().unwrap(),
            "--jobs" => opts.jobs = value().parse().unwrap(),
            "--photo-colors" => p.photo_colors = value().parse().unwrap(),
            "--muted-photo-colors" => p.muted_photo_colors = value().parse().unwrap(),
            "--max-dominant-share" => p.max_dominant_share = value().parse().unwrap(),
            "--gutter-tolerance" => p.gutter_tolerance = value().parse().unwrap(),
            "--gutter-slack" => p.gutter_slack = value().parse().unwrap(),
            "--min-side" => p.min_side = value().parse().unwrap(),
            "--min-photo-fill" => p.min_photo_fill = value().parse().unwrap(),
            "--edge-contrast" => p.edge_contrast = value().parse().unwrap(),
            "--min-edge-run" => p.min_edge_run = value().parse().unwrap(),
            "--edge-gap" => p.edge_gap = value().parse().unwrap(),
            "--screen" => {
                opts.screen = match value().as_str() {
                    "desktop" => Some(Screen::Desktop),
                    "phone" => Some(Screen::Phone),
                    other => panic!("--screen is desktop or phone, not {other}"),
                }
            }
            _ if arg.starts_with("--") => panic!("unknown option {arg}"),
            _ => opts.inputs.push(PathBuf::from(arg)),
        }
    }
    opts
}

fn main() {
    let opts = parse_args();
    if opts.bench {
        return bench(&opts.model);
    }
    if opts.probe {
        let model = opts.model.load();
        for path in &opts.inputs {
            let (out, _) = model.view(&image::open(path).expect("decode image"));
            println!(
                "{}",
                json!({ "file": path.display().to_string(), "view": out })
            );
        }
        return;
    }
    collect(&opts);
}

fn collect(opts: &Options) {
    let out_dir = opts.out.as_ref().expect("--out is required");
    let input = opts
        .inputs
        .first()
        .expect("a screenshots directory is required");
    let mut files: Vec<PathBuf> = fs::read_dir(input)
        .expect("read screenshots directory")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .collect();
    files.sort();
    fs::create_dir_all(out_dir.join("blur")).expect("create output directory");

    let p = &opts.params;
    let meta = json!({
        "model": opts.model.meta(),
        "max_regions_collected": opts.max_regions,
        "extract_params": {
            "photo_colors": p.photo_colors,
            "muted_photo_colors": p.muted_photo_colors,
            "max_dominant_share": p.max_dominant_share,
            "gutter_tolerance": p.gutter_tolerance,
            "gutter_slack": p.gutter_slack,
            "min_side": p.min_side,
            "min_photo_fill": p.min_photo_fill,
            "edge_contrast": p.edge_contrast,
            "min_edge_run": p.min_edge_run,
            "edge_gap": p.edge_gap,
        },
        "extract_params_are_default": *p == ExtractParams::default(),
        "grayscale": opts.grayscale,
        "scoring_constants": scoring_constants()
            .into_iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect::<serde_json::Map<_, _>>(),
    });
    fs::write(
        out_dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap(),
    )
    .unwrap();

    // Records come back in whatever order the workers finish; they're sorted before writing.
    let records = Mutex::new(Vec::new());
    let next = Mutex::new(0usize);
    let classifier = RiskClassifier::new(MODEL_BYTES);
    std::thread::scope(|scope| {
        for _ in 0..opts.jobs.min(files.len()).max(1) {
            scope.spawn(|| {
                let model = opts.model.load();
                loop {
                    let i = {
                        let mut n = next.lock().unwrap();
                        *n += 1;
                        *n - 1
                    };
                    let Some(path) = files.get(i) else { break };
                    let rec = record(path, opts, out_dir, &model, &classifier);
                    eprintln!("[{}/{}] {}", i + 1, files.len(), rec["file"]);
                    records.lock().unwrap().push(rec);
                }
            });
        }
    });

    let mut records = records.into_inner().unwrap();
    records.sort_by(|a, b| a["file"].as_str().cmp(&b["file"].as_str()));
    let mut out = fs::File::create(out_dir.join("shots.jsonl")).unwrap();
    for rec in &records {
        writeln!(out, "{rec}").unwrap();
    }
    eprintln!(
        "wrote {} records to {}",
        records.len(),
        out_dir.join("shots.jsonl").display()
    );
}

/// One screenshot's record:
///
/// - `file`, `width`, `height`
/// - `whole`: `{skin, gray, ...model outputs}` for the full frame
/// - `regions`: up to `max_regions` of `{x, y, width, height, skin, gray, blur, ...model
///   outputs}`, largest first
/// - `rust`: what `classify_regions` returns with the built-in constants (gantman with default
///   extraction parameters only, so the scorer can check it matches)
/// - `timing_ms`: `{extract, model_runs, model_ms}`
/// - `blur`: path of the blurred screenshot, relative to the output directory
fn record(
    path: &Path,
    opts: &Options,
    out_dir: &Path,
    model: &Model,
    classifier: &RiskClassifier,
) -> Value {
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    let mut img = image::open(path).expect("decode screenshot");
    if opts.grayscale {
        img = DynamicImage::ImageRgb8(DynamicImage::ImageLuma8(img.to_luma8()).to_rgb8());
    }
    let (width, height) = img.dimensions();
    let screen = opts.screen.unwrap_or(if height > width {
        Screen::Phone
    } else {
        Screen::Desktop
    });

    let start = Instant::now();
    let regions = extract_regions_tagged(&img, opts.max_regions, &opts.params);
    let extract_ms = start.elapsed().as_secs_f64() * 1000.0;

    let mut model_runs = 0;
    let model_start = Instant::now();
    let (mut whole, runs) = model.view(&img);
    model_runs += runs;
    whole.insert("skin".into(), json!(skin_score(&img)));
    whole.insert("gray".into(), json!(is_grayscale(&img)));

    let mut region_values = Vec::new();
    for (n, e) in regions.iter().enumerate() {
        let (r, c) = (&e.region, &e.container);
        let crop = img.crop_imm(r.x, r.y, r.width, r.height);
        let (mut value, runs) = model.view(&crop);
        model_runs += runs;
        let blur = format!("blur/{stem}_r{}.webp", n + 1);
        write_blurred(crop.clone(), &out_dir.join(&blur));
        for (k, v) in [
            ("x", json!(r.x)),
            ("y", json!(r.y)),
            ("width", json!(r.width)),
            ("height", json!(r.height)),
            ("skin", json!(skin_score(&crop))),
            ("gray", json!(is_grayscale(&crop))),
            ("blur", json!(blur)),
            ("container", json!([c.x, c.y, c.width, c.height])),
        ] {
            value.insert(k.into(), v);
        }
        region_values.push(Value::Object(value));
    }
    let model_ms = model_start.elapsed().as_secs_f64() * 1000.0;

    let rust = match (&opts.model, opts.params == ExtractParams::default()) {
        (ModelSpec::Gantman, true) => {
            let r = classifier
                .classify_regions(&img, screen)
                .expect("classify_regions");
            json!({
                "risk": r.scores.risk,
                "nsfw": r.scores.nsfw,
                "skin": r.scores.skin,
                "model_runs": r.model_runs,
                "region_scores": r.regions.iter().map(|x| x.score).collect::<Vec<_>>(),
            })
        }
        _ => Value::Null,
    };

    let blur = format!("blur/{stem}.webp");
    write_blurred(img, &out_dir.join(&blur));
    json!({
        "file": format!("{stem}.png"),
        "screen": match screen {
            Screen::Desktop => "desktop",
            Screen::Phone => "phone",
        },
        "width": width,
        "height": height,
        "whole": whole,
        "regions": region_values,
        "rust": rust,
        "timing_ms": { "extract": extract_ms, "model_runs": model_runs, "model_ms": model_ms },
        "blur": blur,
    })
}

fn write_blurred(img: DynamicImage, path: &Path) {
    let processed = ImagePipeline.process_image(img, 0).expect("blur");
    fs::write(path, processed.bytes).expect("write blurred image");
}

/// Single-threaded timings and memory for one model, as JSON on stdout. Run it in its own
/// process per model so the peak-memory numbers belong to that model alone.
fn bench(spec: &ModelSpec) {
    // A noisy 640×400 "photo", a typical region size.
    let mut state = 0x2545_f491u32;
    let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(640, 400, |x, y| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let n = (state % 40) as u8;
        image::Rgb([(x / 3) as u8 / 2 + n, (y / 2) as u8 / 2 + n, 120 + n])
    }));
    let rss_base = rss_mb("VmRSS");
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };

    let mut loads = Vec::new();
    let mut model = None;
    for _ in 0..5 {
        let t = Instant::now();
        model = Some(spec.load());
        loads.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let model = model.unwrap();
    let rss_loaded = rss_mb("VmRSS");

    model.view(&img);
    let mut views = Vec::new();
    let mut runs = 0;
    for _ in 0..10 {
        let t = Instant::now();
        runs = model.view(&img).1;
        views.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let view_ms = median(views);
    println!(
        "{}",
        json!({
            "model": spec.meta(),
            "load_ms": median(loads),
            "view_ms": view_ms,
            "runs_per_view": runs,
            "run_ms": view_ms / runs as f64,
            "rss_base_mb": rss_base,
            "rss_after_load_mb": rss_loaded,
            "rss_peak_mb": rss_mb("VmHWM"),
        })
    );
}

fn rss_mb(field: &str) -> f64 {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find(|l| l.starts_with(field))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|kb| kb.parse::<f64>().ok())
        .map_or(0.0, |kb| kb / 1024.0)
}
