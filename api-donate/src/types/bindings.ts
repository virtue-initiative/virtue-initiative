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
  // Unset in local dev, where confirmation links are logged instead of emailed.
  AWS_ACCESS_KEY_ID?: string;
  AWS_SECRET_ACCESS_KEY?: string;
}

export interface Variables {}
