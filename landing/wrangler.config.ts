import { defineWranglerConfig } from 'wrangler/experimental-config';

// Settings for Wrangler, which cf runs to deploy this Worker.
export default defineWranglerConfig({ assetsDirectory: './dist' });
