# Tampering Detection

Canonical spec for `LifecycleModule` (`client/core/src/module/lifecycle.rs`).
For a diagrammed walkthrough see the developer doc at
`landing/src/content/help/developer/lifecycle.md`.

## Model

The monitoring process is expected to run for a known window — login →
logout. We detect any stretch of that window it wasn't running, excluding
suspend, which is identified by comparing a boot clock (includes suspend)
against a monotonic clock (excludes suspend) each `Ping`, not by subscribing
to OS sleep/wake events.

## Events / log entries (closed set — exactly these seven)

Informational (risk 0.0):

- `SuspendDetected { duration_ms }` — a suspend interval detected
  retrospectively via boot-vs-monotonic clock divergence
- `SystemLogin { utc_ms }` — start of a new expected-running window
- `SystemLogout { utc_ms }` — end of an expected-running window

Alerts:

- `UnexpectedStart` (risk `HIGH_RISK_LIFECYCLE_ALERT`, batched) — awake time
  between a known login and the first heartbeat sample since
- `UnexpectedStop` (risk `HIGH_RISK_LIFECYCLE_ALERT`, batched) — gap between
  the last known-alive sample and the session's logout
- `UnexpectedGap` (risk `HIGH_RISK_LIFECYCLE_ALERT`, batched) — awake time
  between two consecutive samples in the same boot with no sample
- `UserStop` (risk `EXTRA_HIGH_RISK`, immediate/emailed) — the user
  explicitly quit the monitor while it was expected to be running

`ProcessStarted`/`ProcessStopped` still exist as internal events (driving
when to poll the login/logout hooks, detecting an explicit user stop) but
produce no log row of their own.

## Rules

- Suspend is excused by construction: the monotonic clock doesn't advance
  during suspend, so a mid-session `Δmonotonic` never includes suspended
  time. No separate suspend-tracking state is needed.
- Reboots reset the boot-relative clocks; a boot-clock value smaller than the
  last recorded one is the reboot signal. Any math spanning a reboot anchors
  on UTC + the login/logout hooks instead.
- Each of the three gap buckets (`UnexpectedGap`/`UnexpectedStart`/
  `UnexpectedStop`) uses a shared sliding-window budget: gaps are summed over
  a 10-minute window, and an alert only fires once the summed gap time
  crosses a budget (60s), with a 5-minute cooldown between repeat alerts per
  bucket. A single stall shouldn't alert on its own.
- A reconstructed (unclean-shutdown) logout timestamp is a _floor_, not
  exact — it sits at or before the true end, so the computed
  `UnexpectedStop` gap can only shrink, never be invented. A simultaneous
  force-kill + power-pull correctly produces ~0 gap and stays silent; that's
  intentional, not a detection gap (the machine was down, so no monitored
  activity was possible either way).
- `UserStop` and a later `UnexpectedStop` for the same incident are
  independent — a user-initiated stop doesn't suppress or excuse the
  eventual gap evaluation once the real logout timestamp arrives.

## Known limits (accepted under the current threat model)

- **Quit-and-never-restart** produces no next-startup, so it's invisible to
  this model locally. Caught only by heartbeat silence — a partner noticing
  logs stop arriving.
- **Client-stored state is trusted.** Not defending against local tampering
  yet; that's a separate follow-up.
