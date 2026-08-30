import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';

const DEFAULT_API_URL = 'http://localhost:8787';

export const API_URL =
  (import.meta.env.PUBLIC_API_URL || DEFAULT_API_URL) + `/${CURRENT_API_VERSION}`;
