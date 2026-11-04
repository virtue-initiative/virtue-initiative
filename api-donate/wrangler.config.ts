import { defineWranglerConfig } from 'wrangler/experimental-config';

// Settings for Wrangler, which cf runs to build, serve and deploy this Worker. cf dev can't
// pass --ip on, so DEV_IP sets the address local dev listens on (launch.sh --host).
export default defineWranglerConfig({
  dev: { port: 8788, ...(process.env.DEV_IP && { ip: process.env.DEV_IP }) },
});
