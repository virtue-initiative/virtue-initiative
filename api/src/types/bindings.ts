// Cloudflare Workers environment bindings
export interface Env {
  // D1 Database
  DB: D1Database;

  // R2 Bucket (native Workers binding)
  BUCKET: R2Bucket;

  // JWT Configuration
  JWT_SECRET: string;
}

// Variables stored in context
export interface Variables {
  userId: string;
  deviceId?: string;
}
