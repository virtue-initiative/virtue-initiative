# NSFW screenshot model

`nsfw_small_v1.nnef.tar` is the only model shipped here. It is tracked by Git
LFS and embedded into every client binary via `include_bytes!` in
`src/module/screenshot.rs`; `build.rs` fails the build if it is still an
unresolved LFS pointer.

## Where it came from

It was converted offline from a source ONNX model, `nsfw_small_v1.onnx`, by a
one-off example, `examples/onnx_to_nnef.rs`. That example read the ONNX file
with `tract-onnx`, called `into_typed()`, and wrote the result out as an NNEF
tar with `tract-nnef`.

The conversion stopped at `into_typed()` on purpose. `into_optimized()` fuses
ops (for example `OptMulByScalar`) that NNEF's serializer has no writer for, so
the write fails with "No serializer found for node ... OptMulByScalar".
Optimization happens at load time instead, in `RiskClassifier::new`.

## Both were removed

The ONNX file and the converter were deleted from `HEAD`, along with the
`tract-onnx` dev-dependency. Nothing in the shipped runtime used either one, and
together they added about 17 MB of Git LFS traffic to every CI job that checked
out LFS content.

They are still recoverable from history. Find the deleting commit:

```
git log --diff-filter=D --oneline -- client/core/models/nsfw_small_v1.onnx
```

Then restore the converter and the model from its parent:

```
git show <sha>^:client/core/examples/onnx_to_nnef.rs > client/core/examples/onnx_to_nnef.rs
git checkout <sha>^ -- client/core/models/nsfw_small_v1.onnx
git lfs pull --include client/core/models/nsfw_small_v1.onnx
```

Restoring the converter also means restoring `tract-onnx = "0.21"` under
`[dev-dependencies]` in `client/core/Cargo.toml`.
