# The Virtue Initiative

This is the main repository for The Virtue Initiative, which aims to provide
free tools and resources for accountability. This repository contains all the
code for the websites and client apps. This project is not production ready.
Consider it alpha stage software. We have a working prototype, but not much
else. See the feature table on [the homepage](https://virtueinitiative.org) for
more information about the current state.

## Structure

| Folder                          | What                                                                                       | Live at                          |
| ------------------------------- | ------------------------------------------------------------------------------------------ | -------------------------------- |
| [`web/`](web)                   | The web app, where people manage partners and read logs                                    | https://app.virtueinitiative.org |
| [`landing/`](landing)           | The website, downloads and help pages                                                      | https://virtueinitiative.org     |
| [`api/`](api)                   | The API, a Cloudflare Worker                                                               | https://api.virtueinitiative.org |
| [`hash-server/`](hash-server)   | The hash server that keeps batches tamper-evident                                          |                                  |
| [`api-donate/`](api-donate)     | The donations Worker                                                                       |                                  |
| [`client/`](client)             | The monitoring apps for Linux, macOS, Windows, Android and iOS, and their shared Rust core |                                  |
| [`shared-web/`](shared-web)     | UI components and API types shared by `web/` and `landing/`                                |                                  |
| [`batch-format/`](batch-format) | The encrypted batch format and its test vectors                                            |                                  |
| [`docs/`](docs)                 | Docs that span components                                                                  |                                  |

## Documentation

- [`docs/overview.md`](docs/overview.md): how the system works, end to end.
- [`docs/README.md`](docs/README.md): every doc and what it covers.
- [`docs/development.md`](docs/development.md): setting up a machine and running the stack.
- Each folder's `README.md`: building and running that part.
- Each folder's `SPEC.md`: exactly what that part must do.

## Local development

This project uses [Bun](https://bun.sh) and [just](https://just.systems).

```
git lfs install && git lfs pull      # the NSFW model the clients need
./scripts/setup.sh                   # dependencies, config, local database, Caddy
git config --local core.hookspath scripts/hooks
./scripts/launch.sh                  # API, web, landing and hash server, on ports it prints
```

Then log in to the web app as `dev@dev.com` / `devpassword` after running `just seed`.
`just --list` shows the everyday commands. [`docs/development.md`](docs/development.md) has the
details, including HTTPS local domains, and [`client/README.md`](client/README.md) covers
pointing a client app at your local API.

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
