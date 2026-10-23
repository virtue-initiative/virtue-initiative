export interface Env {
  DB: D1Database;
  STRIPE_SECRET_KEY: string;
  STRIPE_WEBHOOK_SECRET: string;
  // Base URL of the landing site, used to build Checkout success/cancel URLs.
  LANDING_URL: string;
  // Unset in local dev, which skips the Buttondown call (the region is still recorded).
  BUTTONDOWN_API_KEY?: string;
}

export interface Variables {}
