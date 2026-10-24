export interface Env {
  DB: D1Database;
  STRIPE_SECRET_KEY: string;
  STRIPE_WEBHOOK_SECRET: string;
  // Base URL of the landing site, used to build Checkout success/cancel URLs.
  LANDING_URL: string;
  // Signs newsletter confirm and unsubscribe links. The send script needs the same value.
  NEWSLETTER_TOKEN_SECRET: string;
  // Full From header for newsletter mail, ideally on a subdomain separate from account email.
  NEWSLETTER_FROM_EMAIL: string;
  AWS_SES_REGION: string;
  // SES configuration set for newsletter mail. Its SNS event destination feeds POST /newsletter/sns.
  NEWSLETTER_SES_CONFIG_SET: string;
  // Comma-separated SNS topic ARNs accepted by POST /newsletter/sns. Unset rejects every request.
  SNS_TOPIC_ARNS?: string;
  // Checked on every newsletter form post. Unset rejects every post. Local dev uses Cloudflare's
  // always-passing test secret (see .dev.vars.example).
  TURNSTILE_SECRET_KEY?: string;
  // Comma-separated landing hostnames a Turnstile token may have been issued on. When set, the
  // action must also be the newsletter form's. Unset (tests, local dev, where Cloudflare's test
  // keys return fixed values) skips both checks.
  TURNSTILE_HOSTNAMES?: string;
  // Per-IP limit on newsletter form posts. Unset (tests, local dev) skips the check.
  NEWSLETTER_RATE_LIMITER?: RateLimit;
  // Most confirmation emails the form may send in a day, across all addresses. Defaults to 200.
  NEWSLETTER_DAILY_CONFIRMATION_CAP?: string;
  // Where replies to newsletter mail go, since the From address has no mailbox.
  NEWSLETTER_REPLY_TO?: string;
  // Unset only in local dev (LANDING_URL on localhost), where confirmation links are logged
  // instead of emailed. Anywhere else, unset means a send failure.
  AWS_ACCESS_KEY_ID?: string;
  AWS_SECRET_ACCESS_KEY?: string;
}

export interface Variables {}
