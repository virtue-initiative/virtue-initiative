# 0003: Client API URL and intervals are fixed at compile time

**Status:** Accepted

## Context

The clients used to read the API URL, capture interval and batch window from a config file.
On a monitoring client, anything the monitored person can edit is a way around it: point the
client at a dead URL, or set the interval to a day.

## Decision

`client/core/build.rs` bakes these values into the binary. They come from environment
variables, then the repo-root `.env`, then `~/.config/virtue-dev.env`, and the build fails if
an interval is below its minimum. Release builds take the API URL from the release channel, so a
`main` build talks to production and a `staging` build to staging.

## Consequences

- Testing against a local API means rebuilding the client (see `client/README.md`).
- A build's API URL can't be changed after the fact, so a staging build must never reach
  production users. On iOS this is enforced by TestFlight group membership
  (`client/ios/RELEASING.md`).
- There is no config file for support to ask about; `virtue status --advanced` and Status
  Details show the values a build was made with.
