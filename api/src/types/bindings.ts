// Cloudflare Workers environment bindings
export interface Env {
  // D1 Database
  DB: D1Database;

  // R2 Bucket (native Workers binding)
  BUCKET: R2Bucket;

  // JWT Configuration
  JWT_SECRET: string;

  // Allowed CORS origin (e.g. https://app.example.com). Defaults to localhost in dev.
  CORS_ORIGIN: string;
}

// Variables stored in context
export interface Variables {
  userId: string;
  deviceId?: string;
}
