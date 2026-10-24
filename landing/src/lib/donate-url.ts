const DEFAULT_DONATE_API_URL = 'http://localhost:8788';

// Base URL of the api-donate worker that creates Stripe Checkout sessions.
export const DONATE_API_URL = import.meta.env.PUBLIC_DONATE_API_URL || DEFAULT_DONATE_API_URL;

// Public Cloudflare Turnstile site key for the newsletter form. In dev it falls back to
// Cloudflare's always-passing test key, which pairs with the test secret in api-donate's
// .dev.vars.example. A production build without PUBLIC_TURNSTILE_SITE_KEY has no widget, so the
// donate worker rejects every signup.
export const TURNSTILE_SITE_KEY =
  import.meta.env.PUBLIC_TURNSTILE_SITE_KEY ||
  (import.meta.env.DEV ? '1x00000000000000000000AA' : '');

// Stripe-hosted Customer Portal login page. Donors enter their email and Stripe
// emails them a magic link to manage their donations -- no server session needed.
export const STRIPE_PORTAL_URL = import.meta.env.PUBLIC_STRIPE_PORTAL_URL || '';
