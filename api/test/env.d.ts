declare module 'cloudflare:test' {
  interface ProvidedEnv extends Cloudflare.Env {
    JWT_PRIVATE_KEY: string;
    JWT_PUBLIC_KEY: string;
  }
}
