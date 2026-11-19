# 0002: The client core is one sequential loop

**Status:** Accepted (#581, August 2026)

## Context

The core used to be a set of observers passing events to each other, with several threads and
locks, and a detailed tamper model that tracked suspend, reboot and gap buckets separately. It
was hard to reason about, races were hard to test, and the platforms each grew their own
plumbing around it.

## Decision

The core runs one loop (`client/core/src/daemon.rs`). Each tick clones the state, runs every
module in a fixed order with no locks held, and writes the state back. Requests from the UI go
through a channel the loop drains, so only the loop thread changes state. Every platform runs
this same loop; they differ only in how their UI reaches it.

Tamper detection was simplified to one measure: how late each wakeup was against its schedule
(CORE-002).

## Consequences

- The whole daemon can be tested deterministically through the `Scenario` harness, which wraps
  a real `Daemon` with a mock clock and API.
- Long network calls delay the next tick. That is acceptable at the core's cadence of minutes.
- The tamper model no longer tells partners why a device was gone (crash, reboot or kill),
  only that it was. `client/core/tampering.md` lists what it can't catch.
- Older batches still contain the retired lifecycle event types, so the web app keeps
  displaying them.
