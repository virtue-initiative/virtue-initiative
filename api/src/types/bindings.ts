export interface Env {
  DB: D1Database;
  BUCKET: R2Bucket;
  JWT_SECRET: string;
  CORS_ORIGIN: string;
  HASH_SERVER_URL?: string;
  R2_URL: string;
}

export interface Variables {
  sub: string;
}
