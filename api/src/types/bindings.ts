export interface Env {
  DB: D1Database;
  BUCKET: R2Bucket;
  JWT_SECRET: string;
  API_BASE_PATH?: string;
  HASH_SERVER_URL?: string;
  R2_URL: string;
  APP_URL: string;
  APP_NAME: string;
  AWS_ACCESS_KEY_ID: string;
  AWS_SECRET_ACCESS_KEY: string;
  AWS_SES_REGION: string;
  AWS_SES_FROM_EMAIL: string;
  EMAIL_DELIVERY_MODE: 'ses' | 'log';
}

export interface Variables {
  sub: string;
}
