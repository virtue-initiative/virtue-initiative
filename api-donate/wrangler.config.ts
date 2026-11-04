import { defineWranglerConfig } from 'wrangler/experimental-config';

// Settings for Wrangler, which cf runs to build, serve and deploy this Worker.
export default defineWranglerConfig({ dev: { port: 8788 } });
