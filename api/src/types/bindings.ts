export interface Env {
  // A new secret also goes in scripts/check-deploy-secrets.ts, which checks them before deploys.
  DB: D1Database;
  // The donations worker's database (api-donate/), which holds the newsletter list.
  DONATE_DB: D1Database;
  BUCKET: R2Bucket;
  RATE_LIMITER: RateLimit;
  JWT_PRIVATE_KEY: string;
  JWT_PUBLIC_KEY: string;
  API_BASE_PATH?: string;
  COMMIT_SHA?: string;
  HASH_SERVER_URL: string;
  R2_URL: string;
  // 'true' serves GET /r2/* (API-040). Only local dev sets it.
  DEV_R2_ROUTE?: string;
  APP_URL: string;
  LANDING_URL: string;
  APP_NAME: string;
  AWS_ACCESS_KEY_ID: string;
  AWS_SECRET_ACCESS_KEY: string;
  AWS_SES_REGION: string;
  AWS_SES_FROM_EMAIL: string;
  EMAIL_DELIVERY_MODE: 'ses' | 'log';
  BUG_REPORT_EMAIL: string;
  // Comma-separated SNS topic ARNs accepted by POST /email/sns (API-041). Unset rejects all.
  SNS_TOPIC_ARNS?: string;
  // Cloudflare Turnstile secret for the signup and newsletter forms. Unset rejects them.
  TURNSTILE_SECRET_KEY?: string;
  // HMAC key for the made-up salt of an unknown email (API-015). Unset falls back to a random
  // salt. Rotating it changes every made-up salt but no real one, which shows anyone who saved
  // salts which were real, so rotate it only after a leak.
  LOGIN_SALT_SECRET?: string;
}

export interface Variables {
  sub: string;
  // Hash of the refresh token that authenticated this request; set by the session middlewares.
  sessionTokenHash: string;
}
