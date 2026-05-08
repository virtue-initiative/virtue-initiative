export interface WebpDimensions {
  width: number;
  height: number;
}

export function decodeWebpDimensions(bytes: Uint8Array): WebpDimensions | undefined {
  // Need at least 12 bytes for RIFF....WEBP
  if (bytes.length < 12) return undefined;

  // Validate RIFF signature
  if (bytes[0] !== 0x52 || bytes[1] !== 0x49 || bytes[2] !== 0x46 || bytes[3] !== 0x46) {
    return undefined;
  }
  // Validate WEBP signature at bytes 8–11
  if (bytes[8] !== 0x57 || bytes[9] !== 0x45 || bytes[10] !== 0x42 || bytes[11] !== 0x50) {
    return undefined;
  }

  // Need at least 16 bytes for chunk type
  if (bytes.length < 16) return undefined;

  const chunk = String.fromCharCode(bytes[12], bytes[13], bytes[14], bytes[15]);

  if (chunk === "VP8 ") {
    // Need bytes 0–29 (30 bytes)
    if (bytes.length < 30) return undefined;
    // Validate VP8 sync code at bytes 23–25
    if (bytes[23] !== 0x9d || bytes[24] !== 0x01 || bytes[25] !== 0x2a) return undefined;
    const width = (bytes[26] | (bytes[27] << 8)) & 0x3fff;
    const height = (bytes[28] | (bytes[29] << 8)) & 0x3fff;
    return { width, height };
  }

  if (chunk === "VP8L") {
    // Need bytes 0–24 (25 bytes)
    if (bytes.length < 25) return undefined;
    // Verify signature byte at offset 20
    if (bytes[20] !== 0x2f) return undefined;
    const bits =
      bytes[21] |
      (bytes[22] << 8) |
      (bytes[23] << 16) |
      (bytes[24] << 24);
    const width = (bits & 0x3fff) + 1;
    const height = ((bits >>> 14) & 0x3fff) + 1;
    return { width, height };
  }

  if (chunk === "VP8X") {
    // Need bytes 0–29 (30 bytes): w at 24–26, h at 27–29
    if (bytes.length < 30) return undefined;
    const w = bytes[24] | (bytes[25] << 8) | (bytes[26] << 16);
    const h = bytes[27] | (bytes[28] << 8) | (bytes[29] << 16);
    return { width: w + 1, height: h + 1 };
  }

  return undefined;
}
