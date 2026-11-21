import { defineWranglerConfig } from 'wrangler/experimental-config';

// Settings for Wrangler, which cf runs to build, serve and deploy this Worker. cf dev can't
// pass --ip on, so DEV_IP=0.0.0.0 is how to serve local dev to VMs on the network.
export default defineWranglerConfig({
  dev: { port: 8787, ...(process.env.DEV_IP && { ip: process.env.DEV_IP }) },
});
