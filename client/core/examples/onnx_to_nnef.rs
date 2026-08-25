//! One-off conversion tool: reads the embedded ONNX NSFW model, builds and
//! optimizes it via tract-onnx exactly like `RiskClassifier::new` does, then
//! dumps the resulting optimized `TypedModel` to an NNEF tar so the runtime
//! can load it via tract-nnef instead, skipping the ONNX protobuf import.
use std::fs::File;
use std::io::Cursor;

use tract_onnx::prelude::*;

const INPUT_SIZE: i32 = 224;

fn main() -> TractResult<()> {
    let model_bytes = include_bytes!("../models/nsfw_small_v1.onnx");
    let n = INPUT_SIZE;
    // NB: deliberately `into_typed()`, not `into_optimized()` — the optimizer fuses ops (e.g.
    // `OptMulByScalar`) that NNEF's serializer has no writer for ("No serializer found for
    // node ... OptMulByScalar"). Optimization happens at load time instead (see
    // `RiskClassifier::new`), on the NNEF-loaded model.
    let model = tract_onnx::onnx()
        .model_for_read(&mut Cursor::new(model_bytes))?
        .with_input_fact(0, f32::fact([1, n, n, 3]).into())?
        .into_typed()?;

    let out_path = "models/nsfw_small_v1.nnef.tar";
    let f = File::create(out_path)?;
    tract_nnef::nnef().write(&model, f)?;
    let size = std::fs::metadata(out_path)?.len();
    println!("wrote {out_path} ({size} bytes)");
    Ok(())
}
