declare module 'cloudflare:test' {
  interface ProvidedEnv extends Cloudflare.StagingEnv {
    EMAIL_DELIVERY_MODE: 'ses' | 'log';
  }
}
