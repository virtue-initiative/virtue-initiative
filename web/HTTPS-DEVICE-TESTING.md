# HTTPS Testing On Another Device

Use this when you want to open the local web app on a phone, tablet, VM, or
another machine over HTTPS.

## Why this is needed

The web app uses Web Crypto during authentication and encryption setup. In
practice that means the browser needs a secure context.

`http://localhost` is treated as secure by browsers, but
`http://<your-lan-ip>:5173` is not. On mobile Safari this can fail with errors
like:

```text
undefined is not an object (evaluating crypto.subtle.importKey)
```

So for another device you need:

1. The web app over `https://...`
2. The API over `https://...`
3. Matching local env settings so CORS and blob URLs use the tunneled origins

## Install `cloudflared`

For Debian/Ubuntu hosts, use the helper script in this directory:

```bash
cd /home/jeff/code/virtue-initiative/web
bash ./scripts/install-cloudflared.sh
```

## Start local dev servers

In separate terminals:

```bash
cd /home/jeff/code/virtue-initiative/api
npm run dev -- --ip 0.0.0.0 --port 8787
```

```bash
cd /home/jeff/code/virtue-initiative/web
npm run dev -- --host 0.0.0.0
```

## Start Cloudflare quick tunnels

In separate terminals:

```bash
cloudflared tunnel --url http://localhost:8787
```

```bash
cloudflared tunnel --url http://localhost:5173
```

You will get two `https://...trycloudflare.com` URLs:

- one for the API
- one for the web app

## Update local env files

Set the web app to call the tunneled API:

[`web/.env.local`](./.env.local)

```env
VITE_API_URL=https://<api-tunnel-host>
```

Set the API to allow the tunneled web origin and to emit tunneled R2 URLs:

[`api/.dev.vars`](../api/.dev.vars)

```env
CORS_ORIGIN=https://<web-tunnel-host>
R2_URL=https://<api-tunnel-host>/r2
```

Leave this value alone unless you know you need something different:

```env
HASH_SERVER_URL=http://127.0.0.1:8787
```

## Restart after env changes

After editing the env files, restart both dev servers.

For the web server, also allow the generated tunnel hostname. You can do that
either as a one-off shell env var:

```bash
cd /home/jeff/code/virtue-initiative/web
__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS=<web-tunnel-host> \
  npm run dev -- --host 0.0.0.0 --force
```

Or by setting it in [`web/.env.local`](./.env.local):

```env
__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS=<web-tunnel-host>
```

The value can be comma-separated if you need more than one host. `--force` is
useful when changing env or linked package behavior during dev.

## Open the app on the device

Open:

```text
https://<web-tunnel-host>
```

Do not use the local LAN URL on the external device for login testing. That can
fall back into the insecure-context `crypto.subtle` problem.

## Notes

- Quick tunnel URLs change when you restart `cloudflared`.
- When the URL changes, update:
  - [`web/.env.local`](./.env.local)
  - [`api/.dev.vars`](../api/.dev.vars)
  - the `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS` value when starting the web dev server
- If something looks stale, restart the web dev server with `--force`.
