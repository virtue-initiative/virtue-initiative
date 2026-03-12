export interface Env {
  DB: D1Database;
  BUCKET: R2Bucket;
  JWT_SECRET: string;
  CORS_ORIGIN: string;
  HASH_SERVER_URL?: string;
  R2_URL: string;
  APP_URL: string;
  APP_NAME: string;
  AWS_ACCESS_KEY_ID: string;
  AWS_SECRET_ACCESS_KEY: string;
  AWS_SES_REGION: string;
  AWS_SES_FROM_EMAIL: string;
  SES_FROM_NAME?: string;
  EMAIL_DELIVERY_MODE: 'ses' | 'log';
  DEFAULT_CAPTURE_INTERVAL_SECONDS: string;
  TAMPER_WARNING_GAP_HOURS: string;
  TAMPER_CRITICAL_GAP_HOURS: string;
}

export interface Variables {
  sub: string;
}
