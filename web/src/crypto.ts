// Derives a 256-bit AES-GCM key from password using PBKDF2-HMAC-SHA256
// salt = UTF-8 bytes of userId, 100_000 iterations
export async function deriveKey(password: string, userId: string, extractable = false): Promise<CryptoKey> {
  const enc = new TextEncoder();
  const baseKey = await crypto.subtle.importKey(
    'raw',
    enc.encode(password),
    'PBKDF2',
    false,
    ['deriveKey'],
  );
  return crypto.subtle.deriveKey(
    {
      name: 'PBKDF2',
      salt: enc.encode(userId),
      iterations: 100_000,
      hash: 'SHA-256',
    },
    baseKey,
    { name: 'AES-GCM', length: 256 },
    extractable,
    ['decrypt'],
  );
}

// Decrypts AES-256-GCM blob: first 12 bytes = nonce, rest = ciphertext+tag
export async function decryptBatch(key: CryptoKey, data: Uint8Array): Promise<Uint8Array> {
  const nonce = data.slice(0, 12);
  const ciphertext = data.slice(12);
  const plain = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: nonce },
    key,
    ciphertext,
  );
  return new Uint8Array(plain);
}

// Decompresses gzip using native DecompressionStream
export async function decompressGzip(data: Uint8Array): Promise<Uint8Array> {
  const ds = new DecompressionStream('gzip');
  const writer = ds.writable.getWriter();
  const reader = ds.readable.getReader();

  writer.write(data);
  writer.close();

  const chunks: Uint8Array[] = [];
  let totalLength = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    totalLength += value.length;
  }

  const result = new Uint8Array(totalLength);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return result;
}

// SHA-256(prevHash[32] || imageSha256OrZeros[32] || unixMinute_le_u64[8])
// unixMinute = floor(unix_epoch_seconds / 60), written as little-endian u64
export async function computeChainHash(
  prevHash: Uint8Array,
  imageSha256OrZeros: Uint8Array,
  unixMinute: bigint,
): Promise<Uint8Array> {
  const buf = new Uint8Array(32 + 32 + 8);
  buf.set(prevHash, 0);
  buf.set(imageSha256OrZeros, 32);
  const view = new DataView(buf.buffer);
  view.setBigUint64(64, unixMinute, true); // little-endian
  const hash = await crypto.subtle.digest('SHA-256', buf);
  return new Uint8Array(hash);
}
