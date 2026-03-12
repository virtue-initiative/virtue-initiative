import { Env } from '../types/bindings';

/**
 * Put an object directly via the native R2 Workers binding
 */
export async function putObject(
  env: Env,
  key: string,
  body: ReadableStream | ArrayBuffer | string,
  contentType: string,
): Promise<void> {
  await env.BUCKET.put(key, body, {
    httpMetadata: { contentType },
  });
}

/**
 * Check if an object exists via the native R2 Workers binding
 */
export async function objectExists(env: Env, key: string): Promise<boolean> {
  return (await env.BUCKET.head(key)) !== null;
}

/**
 * Get an object from R2 via the native Workers binding
 */
export async function getObject(env: Env, key: string): Promise<R2ObjectBody | null> {
  return env.BUCKET.get(key);
}

export async function deleteObject(env: Env, key: string): Promise<void> {
  await env.BUCKET.delete(key);
}
