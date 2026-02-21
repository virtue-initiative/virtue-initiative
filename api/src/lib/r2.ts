import { S3Client, GetObjectCommand, PutObjectCommand } from '@aws-sdk/client-s3';
import { getSignedUrl } from '@aws-sdk/s3-request-presigner';
import { Env } from '../types/bindings';

function getS3Client(env: Env): S3Client {
  return new S3Client({
    region: 'auto',
    endpoint: `https://${env.R2_ACCOUNT_ID}.r2.cloudflarestorage.com`,
    credentials: {
      accessKeyId: env.R2_ACCESS_KEY_ID,
      secretAccessKey: env.R2_SECRET_ACCESS_KEY,
    },
  });
}

/**
 * Generate a presigned URL for uploading directly to R2 (PUT)
 */
export async function generateUploadUrl(
  env: Env,
  key: string,
  contentType: string,
  expiresIn: number = 300
): Promise<string> {
  const s3 = getS3Client(env);
  return getSignedUrl(
    s3,
    new PutObjectCommand({
      Bucket: env.R2_BUCKET_NAME,
      Key: key,
      ContentType: contentType,
    }),
    { expiresIn }
  );
}

/**
 * Generate a presigned URL for downloading from R2 (GET)
 */
export async function generateDownloadUrl(
  env: Env,
  key: string,
  expiresIn: number = 3600
): Promise<string> {
  const s3 = getS3Client(env);
  return getSignedUrl(
    s3,
    new GetObjectCommand({
      Bucket: env.R2_BUCKET_NAME,
      Key: key,
    }),
    { expiresIn }
  );
}

/**
 * Put an object directly via the native R2 Workers binding
 */
export async function putObject(
  env: Env,
  key: string,
  body: ReadableStream | ArrayBuffer | string,
  contentType: string
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
