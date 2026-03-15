function normalizeApiBasePath(value?: string | null) {
  const trimmed = value?.trim();

  if (!trimmed || trimmed === '/') {
    return null;
  }

  const withLeadingSlash = trimmed.startsWith('/') ? trimmed : `/${trimmed}`;
  const normalized = withLeadingSlash.replace(/\/+$/, '');

  return normalized && normalized !== '/' ? normalized : null;
}

function matchesBasePath(pathname: string, basePath: string) {
  return pathname === basePath || pathname.startsWith(`${basePath}/`);
}

export function stripApiBasePath(pathname: string, configuredBasePath?: string | null) {
  const basePath = normalizeApiBasePath(configuredBasePath);

  if (!basePath || !matchesBasePath(pathname, basePath)) {
    return pathname;
  }

  const strippedPath = pathname.slice(basePath.length);
  return strippedPath || '/';
}

export function getRequestApiBaseUrl(requestUrl: string, configuredBasePath?: string | null) {
  const url = new URL(requestUrl);
  const basePath = normalizeApiBasePath(configuredBasePath);

  if (basePath && matchesBasePath(url.pathname, basePath)) {
    return `${url.origin}${basePath}`;
  }

  return url.origin;
}
