// Cloudflare Workers environment bindings
export interface Env {
  // D1 Database
  DB: D1Database;
  
  // R2 Bucket (native Workers binding)
  BUCKET: R2Bucket;
  
  // R2 S3-compatible API credentials (for presigned URLs)
  R2_ACCOUNT_ID: string;
  R2_ACCESS_KEY_ID: string;
  R2_SECRET_ACCESS_KEY: string;
  R2_BUCKET_NAME: string;

  // JWT Configuration
  JWT_SECRET: string;
  JWT_ACCESS_EXPIRY: string;
  JWT_REFRESH_EXPIRY: string;
}

// Variables stored in context
export interface Variables {
  userId: string;
  deviceId?: string;
}
