export function encodeBase64(
  value: ArrayBuffer | Uint8Array | null | undefined,
): string | undefined {
  if (!value) return undefined;
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
  return Buffer.from(bytes).toString('base64');
}

export function decodeBase64(value: string): ArrayBuffer {
  return Uint8Array.from(Buffer.from(value, 'base64')).buffer;
}

export function encodeHex(value: ArrayBuffer | Uint8Array): string {
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
  return Buffer.from(bytes).toString('hex');
}
