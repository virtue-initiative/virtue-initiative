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

This project uses [Bun](https://bun.sh) as its package manager and runtime.

### First-time setup

The NSFW screenshot model (`client/core/models/*.nnef.tar`) is stored with
[Git LFS](https://git-lfs.com). Install and pull it before building any client,
or the risk classifier silently reports `0` for every screenshot (the client
build now fails loudly if the model is still an unresolved LFS pointer):

```
git lfs install
git lfs pull
```

Run the setup script from the repo root. It installs all dependencies, copies
example config files, runs local database migrations, and installs/configures
[Caddy](https://caddyserver.com) for HTTPS reverse-proxying:

```
./scripts/setup.sh
```

Also configure the git hooks:

```
git config --local core.hookspath scripts/hooks
```

### Starting the dev servers

Use `launch.sh` to start the API, web app, and landing site together with
interleaved, colour-coded logs. Ports are chosen automatically.

**Plain HTTP (simplest):**

```
./scripts/launch.sh
```

**HTTPS via Caddy with a custom local domain** (mimics production URL
structure — useful for testing auth flows, cookies, etc.):

```
./scripts/launch.sh <domain>
```

This registers `<domain>.localhost` (landing) and `app.<domain>.localhost`
(web + API) in the running Caddy instance. Caddy must be running first
(`setup.sh` starts it).

**Client:** Check the build instructions in `client/` and point it at the API
URL printed by `launch.sh` (e.g. `VIRTUE_BASE_API_URL=http://localhost:<port>`).

More information about each component can be found in their respective
subfolders.

## Contributing

If you are interested in contributing, you can reach out to us at
[develop@virtueinitiative.org](mailto:develop@virtueinitiative.org), or you can
create an issue, comment on an issue or create a pull request. We also have a
[Discord](https://discord.gg/4kNsbRuzQD) channel where we discuss development
and can provide help with using the app.

AI is permitted for writing code, but in general not permitted for writing
text. All issues or pull requests should be human-written and site copy should
also be human written, but the code itself can be generated with AI (but it
still will be human reviewed).
