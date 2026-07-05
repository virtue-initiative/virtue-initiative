use std::{path::PathBuf, process};

use virtue_text_detection::{OcrOptions, ScreenshotOCR};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Output {
    Both,
    Text,
    Rects,
}

fn usage(argv0: &str) -> ! {
    eprintln!(
        "Usage: {argv0} <image> [--lang <lang>] [--data <tessdata-dir>] [--output text|rects|both]"
    );
    eprintln!();
    eprintln!("  image                        PNG, JPEG, or any format leptonica supports");
    eprintln!("  --lang  eng                  Tesseract language code (default: eng)");
    eprintln!("  --data  /path                Path to tessdata/ directory (default: system)");
    eprintln!("  --output text|rects|both     What to print (default: both)");
    process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("ocr");

    let mut image_path: Option<PathBuf> = None;
    let mut lang: Option<String> = None;
    let mut data_path: Option<PathBuf> = None;
    let mut output = Output::Both;

    let mut iter = args.iter().skip(1).peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--lang" => {
                lang = Some(
                    iter.next()
                        .unwrap_or_else(|| {
                            eprintln!("--lang requires a value");
                            process::exit(1);
                        })
                        .clone(),
                );
            }
            "--data" => {
                data_path = Some(PathBuf::from(iter.next().unwrap_or_else(|| {
                    eprintln!("--data requires a value");
                    process::exit(1);
                })));
            }
            "--output" => {
                let val = iter.next().unwrap_or_else(|| {
                    eprintln!("--output requires a value");
                    process::exit(1);
                });
                output = match val.as_str() {
                    "text" => Output::Text,
                    "rects" => Output::Rects,
                    "both" => Output::Both,
                    other => {
                        eprintln!("--output must be text, rects, or both; got {other:?}");
                        process::exit(1);
                    }
                };
            }
            path if !path.starts_with('-') => {
                image_path = Some(PathBuf::from(path));
            }
            other => {
                eprintln!("unknown argument: {other}");
                usage(argv0);
            }
        }
    }

    let image_path = image_path.unwrap_or_else(|| usage(argv0));

    let bytes = std::fs::read(&image_path).unwrap_or_else(|e| {
        eprintln!("error reading {}: {e}", image_path.display());
        process::exit(1);
    });

    let ocr = ScreenshotOCR::new(OcrOptions {
        language: lang,
        tesseract_data_path: data_path,
        ..OcrOptions::default()
    })
    .unwrap_or_else(|e| {
        eprintln!("OCR init error: {e}");
        process::exit(1);
    });

    let result = ocr.detect(&bytes).unwrap_or_else(|e| {
        eprintln!("detection error: {e}");
        process::exit(1);
    });

    if result.regions.is_empty() {
        println!("(no text detected)");
        return;
    }

    if output == Output::Text || output == Output::Both {
        if output == Output::Both {
            println!("=== text ===");
        }
        println!("{}", result.text);
    }

    if output == Output::Rects || output == Output::Both {
        if output == Output::Both {
            println!("\n=== rects ({} words) ===", result.regions.len());
        }
        for (i, r) in result.regions.iter().enumerate() {
            let bb = &r.bounding_box;
            println!(
                "[{i:>3}]  {:<40}  x={:.0} y={:.0} w={:.0} h={:.0}",
                format!("{:?}", r.text),
                bb.x,
                bb.y,
                bb.width,
                bb.height,
            );
        }
    }
}
