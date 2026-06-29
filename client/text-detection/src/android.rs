// ML Kit Text Recognition v2 via JNI.  Confidence is not exposed by ML Kit
// v2, so min_word_confidence is ignored on Android.
//
// The Kotlin companion class VirtueOcr.kt (in the Android module) must be
// present on the classpath.  Its recognizeText method returns newline-separated
// records of the form "text|left|top|right|bottom" (pixel ints).

use jni::objects::{JObject, JString, JValue};

use crate::{BoundingBox, OcrError, OcrOptions, OcrResult, TextRegion};

pub struct ScreenshotOCR {
    language: Option<String>,
}

impl ScreenshotOCR {
    pub fn new(options: OcrOptions) -> Result<Self, OcrError> {
        Ok(Self {
            language: options.language,
        })
    }

    pub fn detect(&self, image: &[u8]) -> Result<OcrResult, OcrError> {
        // Obtain the JavaVM from the Android context set by the Activity on startup.
        let ctx = ndk_context::android_context();
        let vm = unsafe {
            jni::JavaVM::from_raw(ctx.vm() as *mut jni::sys::JavaVM)
                .map_err(|e| OcrError::Init(e.to_string()))?
        };
        let mut env = vm
            .attach_current_thread()
            .map_err(|e| OcrError::Init(e.to_string()))?;

        let j_bytes = env
            .byte_array_from_slice(image)
            .map_err(|e| OcrError::Recognition(e.to_string()))?;
        let j_lang = env
            .new_string(self.language.as_deref().unwrap_or(""))
            .map_err(|e| OcrError::Recognition(e.to_string()))?;

        // Call static Kotlin method: VirtueOcr.recognizeText(byte[], String): String
        let result = env
            .call_static_method(
                "com/virtue/client/VirtueOcr",
                "recognizeText",
                "([BLjava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&*j_bytes), JValue::Object(&*j_lang)],
            )
            .map_err(|e| OcrError::Recognition(e.to_string()))?;

        let j_str_obj = result
            .l()
            .map_err(|e| OcrError::Recognition(e.to_string()))?;
        let output = unsafe { env.get_string(&JString::from(j_str_obj)) }
            .map_err(|e| OcrError::Recognition(e.to_string()))?
            .to_string_lossy()
            .into_owned();

        // Parse "text|left|top|right|bottom" records into regions.
        let mut regions = Vec::new();
        let mut text_lines = Vec::new();

        for record in output.split('\n') {
            if record.is_empty() {
                continue;
            }
            let parts: Vec<&str> = record.splitn(5, '|').collect();
            if parts.len() != 5 {
                continue;
            }
            let text = parts[0].to_string();
            if text.is_empty() {
                continue;
            }
            let left: f32 = parts[1].parse().unwrap_or(0.0);
            let top: f32 = parts[2].parse().unwrap_or(0.0);
            let right: f32 = parts[3].parse().unwrap_or(0.0);
            let bottom: f32 = parts[4].parse().unwrap_or(0.0);

            text_lines.push(text.clone());
            regions.push(TextRegion {
                text,
                bounding_box: BoundingBox {
                    x: left,
                    y: top,
                    width: right - left,
                    height: bottom - top,
                },
            });
        }

        Ok(OcrResult {
            regions,
            text: text_lines.join("\n"),
        })
    }
}
