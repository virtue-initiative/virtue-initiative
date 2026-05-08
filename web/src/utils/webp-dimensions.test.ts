import { describe, expect, it } from "vitest";
import { decodeWebpDimensions } from "./webp-dimensions";

function makeVP8(width: number, height: number): Uint8Array {
  // Minimum 30-byte VP8 lossy WebP
  const buf = new Uint8Array(30);
  // RIFF header
  buf[0] = 0x52;
  buf[1] = 0x49;
  buf[2] = 0x46;
  buf[3] = 0x46;
  // WEBP signature
  buf[8] = 0x57;
  buf[9] = 0x45;
  buf[10] = 0x42;
  buf[11] = 0x50;
  // Chunk type "VP8 "
  buf[12] = 0x56;
  buf[13] = 0x50;
  buf[14] = 0x38;
  buf[15] = 0x20;
  // VP8 sync code at bytes 23-25
  buf[23] = 0x9d;
  buf[24] = 0x01;
  buf[25] = 0x2a;
  // Width at 26-27 (14-bit LE), height at 28-29
  buf[26] = width & 0xff;
  buf[27] = (width >> 8) & 0x3f;
  buf[28] = height & 0xff;
  buf[29] = (height >> 8) & 0x3f;
  return buf;
}

function makeVP8L(width: number, height: number): Uint8Array {
  // Minimum 25-byte VP8L lossless WebP
  const buf = new Uint8Array(25);
  buf[0] = 0x52;
  buf[1] = 0x49;
  buf[2] = 0x46;
  buf[3] = 0x46;
  buf[8] = 0x57;
  buf[9] = 0x45;
  buf[10] = 0x42;
  buf[11] = 0x50;
  // Chunk type "VP8L"
  buf[12] = 0x56;
  buf[13] = 0x50;
  buf[14] = 0x38;
  buf[15] = 0x4c;
  // Signature byte at 20
  buf[20] = 0x2f;
  // bits: width-1 in low 14, height-1 in next 14
  const bits = ((width - 1) & 0x3fff) | (((height - 1) & 0x3fff) << 14);
  buf[21] = bits & 0xff;
  buf[22] = (bits >> 8) & 0xff;
  buf[23] = (bits >> 16) & 0xff;
  buf[24] = (bits >> 24) & 0xff;
  return buf;
}

function makeVP8X(width: number, height: number): Uint8Array {
  // Minimum 30-byte VP8X extended WebP
  const buf = new Uint8Array(30);
  buf[0] = 0x52;
  buf[1] = 0x49;
  buf[2] = 0x46;
  buf[3] = 0x46;
  buf[8] = 0x57;
  buf[9] = 0x45;
  buf[10] = 0x42;
  buf[11] = 0x50;
  // Chunk type "VP8X"
  buf[12] = 0x56;
  buf[13] = 0x50;
  buf[14] = 0x38;
  buf[15] = 0x58;
  // width-1 as LE uint24 at 24-26, height-1 at 27-29
  const w = width - 1;
  const h = height - 1;
  buf[24] = w & 0xff;
  buf[25] = (w >> 8) & 0xff;
  buf[26] = (w >> 16) & 0xff;
  buf[27] = h & 0xff;
  buf[28] = (h >> 8) & 0xff;
  buf[29] = (h >> 16) & 0xff;
  return buf;
}

describe("decodeWebpDimensions", () => {
  it("parses VP8 lossy", () => {
    expect(decodeWebpDimensions(makeVP8(1920, 1080))).toEqual({
      width: 1920,
      height: 1080,
    });
  });

  it("parses VP8L lossless", () => {
    expect(decodeWebpDimensions(makeVP8L(800, 600))).toEqual({
      width: 800,
      height: 600,
    });
  });

  it("parses VP8X extended", () => {
    expect(decodeWebpDimensions(makeVP8X(3840, 2160))).toEqual({
      width: 3840,
      height: 2160,
    });
  });

  it("returns undefined for empty buffer", () => {
    expect(decodeWebpDimensions(new Uint8Array(0))).toBeUndefined();
  });

  it("returns undefined for truncated buffer (cut after WEBP)", () => {
    expect(
      decodeWebpDimensions(
        new Uint8Array([
          0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42,
          0x50,
        ]),
      ),
    ).toBeUndefined();
  });

  it("returns undefined for wrong magic (PNG header)", () => {
    const png = new Uint8Array([
      0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
    ]);
    expect(decodeWebpDimensions(png)).toBeUndefined();
  });

  it("returns undefined for unknown chunk type", () => {
    const buf = new Uint8Array(30);
    buf[0] = 0x52;
    buf[1] = 0x49;
    buf[2] = 0x46;
    buf[3] = 0x46;
    buf[8] = 0x57;
    buf[9] = 0x45;
    buf[10] = 0x42;
    buf[11] = 0x50;
    // Chunk type "VP9 "
    buf[12] = 0x56;
    buf[13] = 0x50;
    buf[14] = 0x39;
    buf[15] = 0x20;
    expect(decodeWebpDimensions(buf)).toBeUndefined();
  });

  it("returns undefined for VP8 with bad sync code", () => {
    const buf = makeVP8(640, 480);
    buf[23] = 0x00; // corrupt sync code
    expect(decodeWebpDimensions(buf)).toBeUndefined();
  });
});
