import { CURRENT_API_VERSION } from '../../../shared-web/api-version';

export { CURRENT_API_VERSION };

const VERSION_SEGMENT = /^\/(v\d+(?:\.\d+)?)(\/.*)?$/;

// Requests whose version prefix doesn't match CURRENT_API_VERSION are flagged here so
// that a downstream middleware can respond 410 Gone, since `getPath` can only return a
// rewritten path string, not a Response.
const goneRequests = new WeakSet<Request>();

export function stripApiVersion(pathname: string, request: Request): string {
  const match = pathname.match(VERSION_SEGMENT);
  if (!match) {
    return pathname;
  }

  const [, version, rest] = match;
  if (version !== CURRENT_API_VERSION) {
    goneRequests.add(request);
  }

  return rest || '/';
}

export function isApiVersionGone(request: Request): boolean {
  return goneRequests.has(request);
}
