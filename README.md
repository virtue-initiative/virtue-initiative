# The Virtue Initiative

This is the main repository for The Virtue Initiative, which aims to provide
free tools and resources for accountability. This repository contains all the
code for the websites and client apps. This project is not production ready.
Consider it alpha stage software. We have a working prototype, but not much
else. See the feature table on [the homepage](https://virtueinitiative.org) for
more information about the current state.

## Structure

- [`/web`](./web) contains the code for the main web app. (https://app.virtueinitiative.org)
- [`/landing`](./landing) contains the code for the landing page and help pages. (https://virtueinitiative.org)
- [`/client`](./client) contains subdirectories containing the code for the various desktop/mobile monitoring apps.
- [`/api`](./api) contains the API code that runs on Cloudflare workers. (https://api.virtueinitiative.org)

## Local Development

Generally for local development you will need a client, the web and the API
running. You can start them with the following commands. The web interface will
be at http://localhost:5173.

- **Web**: `cd web && npm install && cp .env.example .env.local && npm run dev`
- **API**: `cd api && npm install && cp .dev.vars.example .dev.vars && npm run
  dev`
- **Client**: Check the build instructions and make sure to set the API URL to
  `http://localhost:8787` (often using an environment variable
  `VIRTUE_BASE_API_URL=http://localhost:8787`)

If you intend to work on the landing page or help pages, you just need that one
site.

```
cd landing
npm install
npm run dev
```

More information about each component can be found in their respective
subfolders.

## Contributing

If you are interested in contributing, you can reach out to us at
[virtue@anb.codes](mailto:virtue@anb.codes), or you can create an issue, comment on an issue or create a
pull request.

AI is permitted for writing code, but in general not permitted for writing
text. All issues or pull requests should be human-written and site copy should
also be human written, but the code itself can be generated with AI (but it
still will be human reviewed).
