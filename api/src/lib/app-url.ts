import { Env } from '../types/bindings';

export function getAppUrl(env: Env): string {
  return env.APP_URL;
}
