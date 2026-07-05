use std::path::PathBuf;

use crate::{BoundingBox, OcrError, OcrOptions, OcrResult, TextRegion};

pub struct ScreenshotOCR {
    lang: String,
    data_path: Option<PathBuf>,
    min_confidence: u8,
}

impl ScreenshotOCR {
    pub fn new(options: OcrOptions) -> Result<Self, OcrError> {
        let lang = options.language.as_deref().unwrap_or("eng");
        let data_path_str = options
            .tesseract_data_path
            .as_deref()
            .and_then(|p| p.to_str());
        leptess::LepTess::new(data_path_str, lang).map_err(|e| OcrError::Init(e.to_string()))?;
        Ok(Self {
            lang: lang.to_string(),
            data_path: options.tesseract_data_path,
            min_confidence: options.min_word_confidence,
        })
    }

    pub fn detect(&self, image: &[u8]) -> Result<OcrResult, OcrError> {
        let data_path_str = self.data_path.as_deref().and_then(|p| p.to_str());

        let mut lt = leptess::LepTess::new(data_path_str, &self.lang)
            .map_err(|e| OcrError::Init(e.to_string()))?;

        // PSM 3 = auto layout analysis (detects columns, sidebars, etc.).
        // leptess doesn't set this explicitly and defaults to PSM 6 (single
        // block), which collapses the whole page into one ocr_carea.
        let _ = lt.set_variable(leptess::Variable::TesseditPagesegMode, "3");

        lt.set_image_from_mem(image)
            .map_err(|e| OcrError::ImageLoad(e.to_string()))?;

        lt.set_source_resolution(72);

        let hocr = lt
            .get_hocr_text(0)
            .map_err(|e| OcrError::Recognition(e.to_string()))?;

        let regions = parse_hocr_words(&hocr);
        let paragraphs = extract_paragraphs(&hocr, self.min_confidence);
        let text = paragraphs.join("\n\n");

        Ok(OcrResult { regions, text })
    }
}

// ── hOCR parser ───────────────────────────────────────────────────────────────
//
// Tesseract hOCR XHTML structure:
//
//   <div class='ocr_page'>
//     <div class='ocr_carea'>
//       <p class='ocr_par'>
//         <span class='ocr_line'>
//           <span class='ocrx_word' title='bbox L T R B; x_wconf N'>TEXT</span>
//         </span>
//       </p>
//     </div>
//   </div>
//
// `parse_hocr_words`  → flat list of all word regions (no confidence filter)
// `extract_paragraphs` → confidence-filtered text lines, one entry per ocr_par

fn parse_hocr_words(hocr: &str) -> Vec<TextRegion> {
    let mut regions = Vec::new();
    let mut pos = 0;

    while pos < hocr.len() {
        let Some(rel) = hocr[pos..].find("<span") else {
            break;
        };
        let tag_start = pos + rel;

        let Some(rel_end) = hocr[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + rel_end;
        let tag = &hocr[tag_start..=tag_end];

        if !tag.contains("ocrx_word") {
            pos = tag_end + 1;
            continue;
        }

        let content_start = tag_end + 1;
        let Some(rel_close) = hocr[content_start..].find("</span>") else {
            break;
        };
        let raw_text = &hocr[content_start..content_start + rel_close];
        let text = decode_entities(strip_tags(raw_text).trim());

        if let Some(bbox) = extract_bbox(tag)
            && !text.is_empty()
        {
            regions.push(TextRegion {
                text,
                bounding_box: bbox,
            });
        }

        pos = content_start + rel_close + "</span>".len();
    }

    regions
}

/// Extract one string per `<p class='ocr_par'>` found in `hocr`.
fn extract_paragraphs(hocr: &str, min_confidence: u8) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut pos = 0;

    while pos < hocr.len() {
        let Some(rel) = hocr[pos..].find("<p ") else {
            break;
        };
        let tag_start = pos + rel;

        let Some(rel_end) = hocr[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + rel_end;
        let tag = &hocr[tag_start..=tag_end];

        if !tag.contains("ocr_par") {
            pos = tag_end + 1;
            continue;
        }

        let content_start = tag_end + 1;
        let Some(rel_close) = hocr[content_start..].find("</p>") else {
            break;
        };
        let par_content = &hocr[content_start..content_start + rel_close];

        let lines = extract_lines(par_content, min_confidence);
        if !lines.is_empty() {
            paragraphs.push(lines.join("\n"));
        }

        pos = content_start + rel_close + "</p>".len();
    }

    paragraphs
}

fn extract_lines(hocr: &str, min_confidence: u8) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pos = 0;

    while pos < hocr.len() {
        let Some(rel) = hocr[pos..].find("<span") else {
            break;
        };
        let tag_start = pos + rel;

        let Some(rel_end) = hocr[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + rel_end;
        let tag = &hocr[tag_start..=tag_end];

        if !tag.contains("ocr_line") {
            pos = tag_end + 1;
            continue;
        }

        let content_start = tag_end + 1;
        let Some((line_body, consumed)) = span_body(&hocr[content_start..]) else {
            break;
        };

        let words = words_from_span(line_body, min_confidence);
        let total_chars: usize = words.iter().map(|w| w.chars().count()).sum();
        if total_chars >= 3 {
            lines.push(words.join(" "));
        }

        pos = content_start + consumed;
    }

    lines
}

fn words_from_span(hocr: &str, min_confidence: u8) -> Vec<String> {
    let mut words = Vec::new();
    let mut pos = 0;

    while pos < hocr.len() {
        let Some(rel) = hocr[pos..].find("<span") else {
            break;
        };
        let tag_start = pos + rel;

        let Some(rel_end) = hocr[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + rel_end;
        let tag = &hocr[tag_start..=tag_end];

        if !tag.contains("ocrx_word") {
            pos = tag_end + 1;
            continue;
        }

        let content_start = tag_end + 1;
        let Some(rel_close) = hocr[content_start..].find("</span>") else {
            break;
        };
        let raw_text = &hocr[content_start..content_start + rel_close];
        let text = decode_entities(strip_tags(raw_text).trim());

        if !text.is_empty() && word_confidence(tag) >= min_confidence {
            words.push(text);
        }

        pos = content_start + rel_close + "</span>".len();
    }

    words
}

// ── Body extractor ────────────────────────────────────────────────────────────

/// Return `(body, bytes_consumed_including_</span>)` handling nested spans.
fn span_body(s: &str) -> Option<(&str, usize)> {
    let mut depth = 1usize;
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with("<span") {
            depth += 1;
            i += 5;
        } else if s[i..].starts_with("</span>") {
            depth -= 1;
            if depth == 0 {
                return Some((&s[..i], i + "</span>".len()));
            }
            i += 7;
        } else {
            match s[i..].find('<') {
                Some(0) => i += 1,
                Some(rel) => i += rel,
                None => break,
            }
        }
    }
    None
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn word_confidence(tag: &str) -> u8 {
    let Some(ci) = tag.find("x_wconf ") else {
        return 100;
    };
    let rest = &tag[ci + "x_wconf ".len()..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().unwrap_or(100)
}

fn decode_entities(s: &str) -> String {
    let s = s
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&#39;", "'");
    decode_numeric_entities(&s)
}

fn decode_numeric_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find("&#") {
        out.push_str(&rest[..amp]);
        rest = &rest[amp + 2..];
        if let Some(semi) = rest.find(';')
            && let Ok(n) = rest[..semi].parse::<u32>()
            && let Some(ch) = char::from_u32(n)
        {
            out.push(ch);
            rest = &rest[semi + 1..];
            continue;
        }
        out.push_str("&#");
    }
    out.push_str(rest);
    out
}

fn extract_bbox(tag: &str) -> Option<BoundingBox> {
    let ti = tag.find("title=")? + "title=".len();
    let rest = &tag[ti..];
    let (quote, rest) = if let Some(rest) = rest.strip_prefix('"') {
        ('"', rest)
    } else if let Some(rest) = rest.strip_prefix('\'') {
        ('\'', rest)
    } else {
        return None;
    };
    let title = &rest[..rest.find(quote)?];
    let bi = title.find("bbox ")? + "bbox ".len();
    let bbox_str = title[bi..].split(';').next()?;
    let mut parts = bbox_str.split_whitespace();
    let left: f32 = parts.next()?.parse().ok()?;
    let top: f32 = parts.next()?.parse().ok()?;
    let right: f32 = parts.next()?.parse().ok()?;
    let bottom: f32 = parts.next()?.parse().ok()?;
    Some(BoundingBox {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HOCR: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<html>
<body>
<div class='ocr_page' id='page_1' title='image unknown; bbox 0 0 800 600; ppageno 0'>
 <div class='ocr_carea' id='block_1_1' title='bbox 0 0 400 300'>
  <p class='ocr_par' id='par_1_1' lang='eng' dir='ltr'>
   <span class='ocr_line' id='line_1_1' title='bbox 0 90 400 130'>
    <span class='ocrx_word' id='word_1_1' title='bbox 36 92 199 122; x_wconf 96'>Hello</span>
    <span class='ocrx_word' id='word_1_2' title='bbox 210 92 310 122; x_wconf 93'>world</span>
   </span>
   <span class='ocr_line' id='line_1_2' title='bbox 0 140 300 180'>
    <span class='ocrx_word' id='word_1_3' title='bbox 36 142 200 172; x_wconf 91'>second</span>
    <span class='ocrx_word' id='word_1_4' title='bbox 210 142 280 172; x_wconf 89'>line</span>
   </span>
  </p>
 </div>
 <div class='ocr_carea' id='block_1_2' title='bbox 450 0 800 300'>
  <p class='ocr_par' id='par_1_2' lang='eng' dir='ltr'>
   <span class='ocr_line' id='line_2_1' title='bbox 450 90 800 130'>
    <span class='ocrx_word' id='word_2_1' title='bbox 450 92 800 122; x_wconf 97'>sidebar</span>
   </span>
  </p>
 </div>
</div>
</body>
</html>"#;

    #[test]
    fn parse_hocr_words_extracts_all_word_regions() {
        let regions = parse_hocr_words(SAMPLE_HOCR);
        assert_eq!(regions.len(), 5);
        assert_eq!(regions[0].text, "Hello");
        assert_eq!(regions[4].text, "sidebar");
    }

    #[test]
    fn extract_paragraphs_joins_text_across_careas() {
        let paragraphs = extract_paragraphs(SAMPLE_HOCR, 0);
        let text = paragraphs.join("\n\n");
        assert_eq!(text, "Hello world\nsecond line\n\nsidebar");
    }

    #[test]
    fn extract_paragraphs_filters_low_confidence() {
        let hocr = r#"<p class='ocr_par'><span class='ocr_line'>
            <span class='ocrx_word' title='bbox 0 0 50 20; x_wconf 90'>good</span>
            <span class='ocrx_word' title='bbox 60 0 90 20; x_wconf 20'>xx</span>
            </span></p>"#;
        let paras = extract_paragraphs(hocr, 50);
        assert_eq!(paras, vec!["good"]);
    }

    #[test]
    fn extract_lines_drops_short_lines() {
        let hocr = r#"
            <span class='ocr_line'>
              <span class='ocrx_word' title='bbox 0 0 5 10; x_wconf 90'>S</span>
            </span>
            <span class='ocr_line'>
              <span class='ocrx_word' title='bbox 0 20 60 40; x_wconf 90'>real</span>
            </span>"#;
        let lines = extract_lines(hocr, 0);
        assert_eq!(lines, vec!["real"]);
    }

    #[test]
    fn decode_entities_handles_named_and_numeric() {
        assert_eq!(decode_entities("&amp;"), "&");
        assert_eq!(decode_entities("&lt;&gt;"), "<>");
        assert_eq!(decode_entities("&quot;"), "\"");
        assert_eq!(decode_entities("&#39;"), "'");
        assert_eq!(
            decode_entities("it&#39;s &amp; &quot;fine&quot;"),
            "it's & \"fine\""
        );
    }

    #[test]
    fn word_confidence_parses_value() {
        assert_eq!(
            word_confidence("<span title='bbox 0 0 10 10; x_wconf 73'>"),
            73
        );
    }

    #[test]
    fn word_confidence_returns_100_when_absent() {
        assert_eq!(word_confidence("<span title='bbox 0 0 10 10'>"), 100);
    }

    #[test]
    fn span_body_handles_nesting() {
        let s = "<span class='ocrx_word'>foo</span><span class='ocrx_word'>bar</span></span>";
        let (body, consumed) = span_body(s).unwrap();
        assert_eq!(
            body,
            "<span class='ocrx_word'>foo</span><span class='ocrx_word'>bar</span>"
        );
        assert_eq!(consumed, s.len());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::OcrOptions;

    #[test]
    #[ignore]
    fn detect_real_image_returns_text_and_regions() {
        let png_path = "/tmp/virtue_ocr_smoke.png";
        std::process::Command::new("convert")
            .args([
                "-size",
                "400x100",
                "xc:white",
                "-font",
                "DejaVu-Sans",
                "-pointsize",
                "36",
                "-fill",
                "black",
                "-annotate",
                "+10+60",
                "Hello OCR",
                png_path,
            ])
            .status()
            .expect("imagemagick not found");

        let png = std::fs::read(png_path).unwrap();
        let ocr = ScreenshotOCR::new(OcrOptions::default()).unwrap();
        let result = ocr.detect(&png).unwrap();

        assert!(!result.regions.is_empty());
        assert!(result.text.to_lowercase().contains("hello"));
    }
}
