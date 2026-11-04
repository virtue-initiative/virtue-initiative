// The Worker's cron triggers, registered by cloudflare.config.ts and dispatched in
// index.ts. Each tick runs only its own case, so the daily snapshot never displaces
// hourly work: 00:05 still fires hourly.
export const HOURLY_CRON = '5 * * * *';
export const DAILY_CRON = '20 0 * * *';
export const CRONS = [HOURLY_CRON, DAILY_CRON];
