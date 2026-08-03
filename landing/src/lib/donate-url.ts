const DEFAULT_DONATE_API_URL = 'http://localhost:8788';

// Base URL of the api-donate worker that creates Stripe Checkout sessions.
export const DONATE_API_URL = import.meta.env.PUBLIC_DONATE_API_URL || DEFAULT_DONATE_API_URL;

// Stripe-hosted Customer Portal login page. Donors enter their email and Stripe
// emails them a magic link to manage their donations -- no server session needed.
export const STRIPE_PORTAL_URL = import.meta.env.PUBLIC_STRIPE_PORTAL_URL || '';
