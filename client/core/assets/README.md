# assets/

## nsfw_model.onnx

MobileNet-based NSFW image classifier in ONNX format.

- **Source**: [infinitered/nsfwjs](https://github.com/infinitered/nsfwjs) converted to ONNX by
  [Fyko/nsfw](https://github.com/Fyko/nsfw)
- **License**: Apache-2.0
- **Version**: v0.2.0 release from https://github.com/Fyko/nsfw/releases/download/v0.2.0/model.onnx
- **Classes**: Drawings, Hentai, Neutral, Porn, Sexy

The model is embedded via `include_bytes!` in `src/nsfw.rs` so the build is hermetic and
requires no runtime download.
